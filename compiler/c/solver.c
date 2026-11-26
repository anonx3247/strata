// Long-lived solver processes for compiler/smt: Z3 started once per core
// (`z3 -in`) and given one script after another, instead of a process per
// script (starting Z3 costs ~8 ms, more than most queries take).
//
// A script runs after `(reset)` (assertions and declarations gone) with the
// limits it was given set again (`:timeout`, `:rlimit`: they outlive a
// reset) and `:print-success` off, then `(echo "@@strata-start")` marks the
// start of its answers and `(echo "@@strata-done")` their end.
// Z3's resource limit counts per `check-sat`, so a script gets the answers
// it gets in a process of its own (the verdict cache relies on that). A
// module parameter (`(set-option :smt.random_seed 3)`,
// `:smt.auto_config`) is global in Z3 and outlives a reset, so a script that
// sets one leaves its process to be stopped (`sets_global`): the next
// script would otherwise be solved under it (a query proven alone gave up
// after an input search's seed). The
// line numbers of error messages are told from the script's first line, as
// a process of its own would. A hard time limit (`-T:S`) kills the process
// (it is started again for the next script) and answers `timeout`, as Z3
// does.
//
// Only Z3 is kept running (`solver_pool_usable`: its `--version` says so):
// the tests' stand-in solvers answer only once their input ends.

#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define POOL_MAX 64
#define DONE "@@strata-done"
#define START "@@strata-start"

typedef struct {
  pid_t pid;      // 0: not running
  int in, out;    // its standard input and output
  long lines;     // lines written to it so far
  int dirty;      // a script set a parameter a reset leaves (`sets_global`)
} Proc;

typedef struct {
  char *buf;
  size_t len, cap;
} Buf;

static Proc procs[POOL_MAX];
// (each thread's own: workers encoding functions ask at the same time)
static __thread char *last_result;
// set when a solver started that didn't answer a command as it read it;
// from then on each script gets a process of its own
static volatile int pool_broken;

static void buf_add(Buf *b, const char *s, size_t n) {
  if (b->len + n + 1 > b->cap) {
    size_t cap = b->cap ? b->cap : 4096;
    while (b->len + n + 1 > cap) cap *= 2;
    char *p = realloc(b->buf, cap);
    if (!p) {
      fputs("fault (Limit): out of memory\n", stderr);
      exit(101);
    }
    b->buf = p;
    b->cap = cap;
  }
  memcpy(b->buf + b->len, s, n);
  b->len += n;
  b->buf[b->len] = 0;
}

static void buf_str(Buf *b, const char *s) { buf_add(b, s, strlen(s)); }

static double now_s(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return (double)t.tv_sec + (double)t.tv_nsec * 1e-9;
}

static void proc_stop(Proc *p) {
  if (p->pid > 0) {
    kill(p->pid, SIGKILL);
    close(p->in);
    close(p->out);
    waitpid(p->pid, NULL, 0);
  }
  p->pid = 0;
  p->lines = 0;
  p->dirty = 0;
}

// At exit, each process is told its input has ended and is waited for, so
// its time counts as this process's children's.
static void pool_end(void);
static void register_end(void) { atexit(pool_end); }

static void pool_end(void) {
  for (int i = 0; i < POOL_MAX; i++) {
    if (procs[i].pid > 0) {
      close(procs[i].in);
      close(procs[i].out);
    }
  }
  for (int i = 0; i < POOL_MAX; i++) {
    if (procs[i].pid > 0) waitpid(procs[i].pid, NULL, 0);
    procs[i].pid = 0;
  }
}

// `solver -in`, its standard input and output as pipes (not inherited by
// what this process starts later), its errors dropped.
static int proc_start(Proc *p, const char *solver) {
  static pthread_once_t once = PTHREAD_ONCE_INIT;
  pthread_once(&once, register_end);
  int in[2], out[2];
  if (pipe(in) != 0) return 0;
  if (pipe(out) != 0) {
    close(in[0]);
    close(in[1]);
    return 0;
  }
  pid_t pid = fork();
  if (pid < 0) {
    close(in[0]);
    close(in[1]);
    close(out[0]);
    close(out[1]);
    return 0;
  }
  if (pid == 0) {
    dup2(in[0], 0);
    dup2(out[1], 1);
    int devnull = open("/dev/null", O_WRONLY);
    if (devnull >= 0) dup2(devnull, 2);
    for (int fd = 3; fd < 1024; fd++) close(fd);
    execl(solver, solver, "-in", (char *)NULL);
    execlp(solver, solver, "-in", (char *)NULL);
    _exit(127);
  }
  close(in[0]);
  close(out[1]);
  fcntl(in[1], F_SETFD, FD_CLOEXEC);
  fcntl(out[0], F_SETFD, FD_CLOEXEC);
  fcntl(in[1], F_SETFL, fcntl(in[1], F_GETFL) | O_NONBLOCK);
  fcntl(out[0], F_SETFL, fcntl(out[0], F_GETFL) | O_NONBLOCK);
  p->pid = pid;
  p->in = in[1];
  p->out = out[0];
  p->lines = 0;
  p->dirty = 0;
  // (it must answer a command as it reads it, within 10 s: else it is no
  // use kept running, and the scripts go to a process each, `pool_broken`)
  const char *ready = "(echo \"@@strata-ready\")\n";
  const char *want = "@@strata-ready\n";
  size_t sent = 0, got = 0;
  char reply[64];
  double deadline = now_s() + 10;
  while (got < strlen(want)) {
    double left = deadline - now_s();
    if (left <= 0) break;
    struct pollfd fds[2] = {{p->out, POLLIN, 0}, {p->in, POLLOUT, 0}};
    int nf = sent < strlen(ready) ? 2 : 1;
    if (poll(fds, nf, (int)(left * 1000) + 1) <= 0) continue;
    if (nf > 1 && (fds[1].revents & POLLOUT)) {
      ssize_t w = write(p->in, ready + sent, strlen(ready) - sent);
      if (w > 0) sent += (size_t)w;
    }
    if (fds[0].revents & (POLLIN | POLLHUP | POLLERR)) {
      ssize_t r = read(p->out, reply + got, sizeof reply - 1 - got);
      if (r <= 0) break;
      got += (size_t)r;
    }
  }
  if (got != strlen(want) || memcmp(reply, want, got) != 0) {
    proc_stop(p);
    pool_broken = 1;
    return 0;
  }
  p->lines = 1;
  return 1;
}

static long count_lines(const char *s, size_t n) {
  long k = 0;
  for (size_t i = 0; i < n; i++)
    if (s[i] == '\n') k++;
  return k;
}

// `out` with each `line N column` told from line `base + 1` (as the
// script's own line 1).
static void fix_lines(Buf *out, const char *s, size_t n, long base) {
  const char *key = "line ";
  size_t i = 0;
  while (i < n) {
    if (n - i > 5 && memcmp(s + i, key, 5) == 0 && s[i + 5] >= '0' && s[i + 5] <= '9') {
      size_t j = i + 5;
      long v = 0;
      while (j < n && s[j] >= '0' && s[j] <= '9') v = v * 10 + (s[j++] - '0');
      if (n - j >= 7 && memcmp(s + j, " column", 7) == 0) {
        char num[32];
        snprintf(num, sizeof num, "line %ld", v - base);
        buf_str(out, num);
        i = j;
        continue;
      }
    }
    buf_add(out, s + i, 1);
    i++;
  }
}

// The answers given so far in this process, by script and flags (a
// script is asked again: termination's loop bounds and the encoder ask the
// same searches): keyed by two 64-bit hashes and the length, kept only for
// a script answered to its end.
typedef struct {
  uint64_t h1, h2;
  size_t len;
  char *answer;
} Memo;

static Memo *memo;
static size_t memo_cap, memo_count;
static pthread_mutex_t memo_lock = PTHREAD_MUTEX_INITIALIZER;

static void memo_hash(const char *flags, const char *script, size_t n, uint64_t *h1, uint64_t *h2) {
  uint64_t a = 1469598103934665603ULL, b = 0x9e3779b97f4a7c15ULL;
  for (const char *f = flags; *f; f++) {
    a = (a ^ (unsigned char)*f) * 1099511628211ULL;
    b = (b ^ (unsigned char)*f) * 0xff51afd7ed558ccdULL + 7;
  }
  a = (a ^ 0x1f) * 1099511628211ULL;
  for (size_t i = 0; i < n; i++) {
    a = (a ^ (unsigned char)script[i]) * 1099511628211ULL;
    b = (b ^ (unsigned char)script[i]) * 0xff51afd7ed558ccdULL + 7;
  }
  *h1 = a;
  *h2 = b;
}

// The answer kept for a script, copied into `result`; 0 when there is none
// (`memo_lock` held).
static int memo_find(uint64_t h1, uint64_t h2, size_t n, Buf *result) {
  int found = 0;
  if (memo_cap) {
    size_t i = (size_t)(h1 % memo_cap);
    while (memo[i].answer) {
      if (memo[i].h1 == h1 && memo[i].h2 == h2 && memo[i].len == n) {
        buf_str(result, memo[i].answer);
        found = 1;
        break;
      }
      i = (i + 1) % memo_cap;
    }
  }
  return found;
}

// The scripts running now (by their hashes and length), at most one per
// worker: a second job of one waits for the first's answer instead of
// running it again (a batch queued ahead by `solver_pool_prefetch`, then
// asked for).
typedef struct {
  uint64_t h1, h2;
  size_t len;
} Running;

static Running running[POOL_MAX];
static int running_count;
static pthread_cond_t running_done = PTHREAD_COND_INITIALIZER;

static int running_has(uint64_t h1, uint64_t h2, size_t n) {
  for (int i = 0; i < running_count; i++)
    if (running[i].h1 == h1 && running[i].h2 == h2 && running[i].len == n) return 1;
  return 0;
}

static void memo_put(uint64_t h1, uint64_t h2, size_t n, const char *answer) {
  pthread_mutex_lock(&memo_lock);
  if (2 * (memo_count + 1) > memo_cap) {
    size_t cap = memo_cap ? 2 * memo_cap : 1024;
    Memo *m = calloc(cap, sizeof(Memo));
    if (!m) {
      pthread_mutex_unlock(&memo_lock);
      return;
    }
    for (size_t k = 0; k < memo_cap; k++) {
      if (memo[k].answer) {
        size_t i = (size_t)(memo[k].h1 % cap);
        while (m[i].answer) i = (i + 1) % cap;
        m[i] = memo[k];
      }
    }
    free(memo);
    memo = m;
    memo_cap = cap;
  }
  size_t i = (size_t)(h1 % memo_cap);
  while (memo[i].answer) {
    if (memo[i].h1 == h1 && memo[i].h2 == h2 && memo[i].len == n) {
      pthread_mutex_unlock(&memo_lock);
      return;
    }
    i = (i + 1) % memo_cap;
  }
  memo[i] = (Memo){h1, h2, n, strdup(answer)};
  memo_count++;
  pthread_mutex_unlock(&memo_lock);
}

// The first line of `out` that isn't `success` (a reply `:print-success`
// asks for): its start in `*at`, its length returned.
static size_t first_answer(const char *out, size_t len, size_t *at) {
  size_t i = 0;
  while (out && i < len) {
    size_t j = i;
    while (j < len && out[j] != '\n') j++;
    if (!(j - i == 7 && memcmp(out + i, "success", 7) == 0)) {
      *at = i;
      return j - i;
    }
    i = j + 1;
  }
  *at = 0;
  return 0;
}

// STRATA_SOLVER_LOG=FILE: each script's time, flags, size, hash and first
// answer, a line each (`again`: answered before, not asked).
static void solver_log(const char *flags, size_t n, uint64_t h, double secs, const char *out, size_t len, const char *note) {
  const char *log = getenv("STRATA_SOLVER_LOG");
  if (!log || !*log) return;
  FILE *lf = fopen(log, "a");
  if (!lf) return;
  size_t at;
  size_t first = first_answer(out, len, &at);
  fprintf(lf, "%.4f\t%s\t%zu\t%016llx\t%.*s%s\n", secs, flags, n, (unsigned long long)h, (int)(first < 60 ? first : 60), out ? out + at : "", note);
  fclose(lf);
}

// STRATA_SOLVER_DUMP=DIR: each script the solver is asked (not answered
// from the memo) as DIR/HASH.smt2, its first answer after it as a comment.
static void solver_dump(const char *script, size_t n, uint64_t h, const char *out, size_t len) {
  const char *dir = getenv("STRATA_SOLVER_DUMP");
  if (!dir || !*dir) return;
  char path[4096];
  snprintf(path, sizeof path, "%s/%016llx.smt2", dir, (unsigned long long)h);
  FILE *f = fopen(path, "w");
  if (!f) return;
  fwrite(script, 1, n, f);
  size_t at;
  size_t first = first_answer(out, len, &at);
  fprintf(f, "\n; answer: %.*s\n", (int)first, out ? out + at : "");
  // and each error the solver reported (compiler/smt's `solver_error`)
  for (size_t i = 0; out && i < len;) {
    size_t j = i;
    while (j < len && out[j] != '\n') j++;
    if (j - i >= 7 && strncmp(out + i, "(error ", 7) == 0) fprintf(f, "; %.*s\n", (int)(j - i), out + i);
    i = j + 1;
  }
  fclose(f);
}

// Whether `flags` (words separated by spaces) hold the word `w`.
static int has_word(const char *flags, const char *w) {
  size_t n = strlen(w);
  const char *f = flags;
  while (*f) {
    while (*f == ' ') f++;
    if (strncmp(f, w, n) == 0 && (f[n] == ' ' || f[n] == 0)) return 1;
    while (*f && *f != ' ') f++;
  }
  return 0;
}

// Whether `script` sets a module parameter (`(set-option :smt.…`: a name
// with a dot), which Z3 keeps across `(reset)`.
static int sets_global(const char *script, size_t n) {
  const char *key = "(set-option :";
  size_t k = strlen(key);
  for (size_t i = 0; i + k <= n; i++) {
    if (memcmp(script + i, key, k) != 0) continue;
    for (size_t j = i + k; j < n; j++) {
      char c = script[j];
      if (c == '.') return 1;
      if (c == ' ' || c == ')' || c == '\n' || c == '\t' || c == '\r') break;
    }
  }
  return 0;
}

// One script's answers from process `p` (started when it isn't running),
// with its flags (`-t:MS`, `-T:S`, `rlimit=N`): what the solver wrote
// before the end mark, or, when it died or ran past `-T`, what it wrote.
static void run_fresh(Proc *p, const char *solver, const char *flags, const char *script, size_t script_len, uint64_t h1, uint64_t h2, Buf *result) {
  long timeout_ms = 4294967295L, rlimit = 0;
  double hard = 0;
  const char *f = flags;
  while (*f) {
    while (*f == ' ') f++;
    if (strncmp(f, "-t:", 3) == 0) timeout_ms = atol(f + 3);
    else if (strncmp(f, "-T:", 3) == 0) hard = atof(f + 3);
    else if (strncmp(f, "rlimit=", 7) == 0) rlimit = atol(f + 7);
    while (*f && *f != ' ') f++;
  }
  // STRATA_SOLVER_TIMEOUT_MS=N: every script's soft time limit (`:timeout`)
  // is N ms, its resource limit unchanged, so a case can make a query stop
  // by time well within its units (compiler/smt's `gave_up` tells the two
  // apart).
  const char *soft = getenv("STRATA_SOLVER_TIMEOUT_MS");
  if (soft && *soft) timeout_ms = atol(soft);
  // (a process left with a global parameter set starts again)
  if (p->dirty) proc_stop(p);
  if (p->pid == 0 && !proc_start(p, solver)) return;
  p->dirty = sets_global(script, script_len);
  // (`:print-success` outlives a reset: the head's own replies end at the
  // START mark, and are dropped, so the script's replies are its own)
  char head[320];
  snprintf(head, sizeof head, "(reset)\n(set-option :timeout %ld)\n(set-option :rlimit %ld)\n(set-option :print-success false)\n(echo \"" START "\")\n", timeout_ms, rlimit);
  long base = p->lines + count_lines(head, strlen(head));
  Buf in = {0};
  buf_str(&in, head);
  buf_add(&in, script, script_len);
  buf_str(&in, "\n(echo \"" DONE "\")\n");
  p->lines += count_lines(in.buf, in.len);
  Buf out = {0};
  size_t sent = 0;
  int done = 0, dead = 0;
  double started = now_s();
  double deadline = hard > 0 ? started + hard : 0;
  while (!done && !dead) {
    struct pollfd fds[2];
    int nf = 0;
    fds[nf++] = (struct pollfd){p->out, POLLIN, 0};
    if (sent < in.len) fds[nf++] = (struct pollfd){p->in, POLLOUT, 0};
    int wait_ms = -1;
    if (deadline > 0) {
      double left = deadline - now_s();
      if (left <= 0) {
        dead = 2;
        break;
      }
      wait_ms = (int)(left * 1000) + 1;
    }
    int r = poll(fds, nf, wait_ms);
    if (r < 0) {
      if (errno == EINTR) continue;
      dead = 1;
      break;
    }
    if (r == 0) continue;
    if (nf > 1 && (fds[1].revents & (POLLOUT | POLLERR | POLLHUP))) {
      ssize_t w = write(p->in, in.buf + sent, in.len - sent);
      if (w > 0) sent += (size_t)w;
      else if (w < 0 && errno != EAGAIN && errno != EINTR) dead = 1;
    }
    if (fds[0].revents & (POLLIN | POLLERR | POLLHUP)) {
      char chunk[65536];
      ssize_t got = read(p->out, chunk, sizeof chunk);
      if (got > 0) {
        buf_add(&out, chunk, (size_t)got);
        size_t m = strlen(DONE) + 1;
        if (out.len >= m && memcmp(out.buf + out.len - m, DONE "\n", m) == 0 &&
            (out.len == m || out.buf[out.len - m - 1] == '\n')) {
          out.len -= m;
          out.buf[out.len] = 0;
          done = 1;
        }
      } else if (got == 0 || (errno != EAGAIN && errno != EINTR)) {
        dead = 1;
      }
    }
  }
  size_t from = 0;
  {
    size_t m = strlen(START) + 1;
    for (size_t i = 0; i + m <= out.len; i++) {
      if ((i == 0 || out.buf[i - 1] == '\n') && memcmp(out.buf + i, START "\n", m) == 0) {
        from = i + m;
        break;
      }
    }
  }
  if (out.len > from) fix_lines(result, out.buf + from, out.len - from, base);
  solver_log(flags, script_len, h1, now_s() - started, out.buf + from, out.len - from, "");
  solver_dump(script, script_len, h1, out.buf + from, out.len - from);
  if (!done && dead == 2) buf_str(result, "timeout\n");
  if (has_word(flags, "budget")) {
    // (a search: how long it took, kept with its answer, so a later ask
    // of it is told the same)
    char took[64];
    snprintf(took, sizeof took, "(:strata-micros %lld)\n", (long long)((now_s() - started) * 1e6));
    buf_str(result, took);
  }
  if (!done) {
    proc_stop(p);
  } else {
    memo_put(h1, h2, script_len, result->buf ? result->buf : "");
  }
  free(in.buf);
  free(out.buf);
}

// The search budget of a run (`solver_budget`): the solver's resource
// units the scripts flagged `budget` (loop-invariant searches) may use in
// all, -1 for no limit; the units they used (each script's `(:rlimit N)`,
// else its whole `rlimit`: it was stopped), and how many were refused once
// it was used up. A script answered before (the memo) costs nothing, so the
// units used are the same whichever worker asks first.
static int64_t budget_units = -1, budget_used, budget_refused;

// The resource units a script's answer says it used (`(:rlimit N)`), else
// its whole limit (`rlimit=N` in its flags).
static int64_t units_used(const char *flags, const Buf *result) {
  const char *at = result->buf ? strstr(result->buf, "(:rlimit ") : NULL;
  if (at) return atoll(at + 9);
  const char *r = strstr(flags, "rlimit=");
  return r ? atoll(r + 7) : 0;
}

// The search budget from now on (`units` < 0: none), nothing used yet.
int64_t solver_budget(int64_t units) {
  __atomic_store_n(&budget_units, units, __ATOMIC_SEQ_CST);
  __atomic_store_n(&budget_used, 0, __ATOMIC_SEQ_CST);
  __atomic_store_n(&budget_refused, 0, __ATOMIC_SEQ_CST);
  return 0;
}

// The units the searches used so far.
int64_t solver_budget_used(void) {
  return __atomic_load_n(&budget_used, __ATOMIC_SEQ_CST);
}

// How many searches were refused, the budget used up.
int64_t solver_budget_refused(void) {
  return __atomic_load_n(&budget_refused, __ATOMIC_SEQ_CST);
}

// `run_fresh`, unless the script was answered before (the memo) or is
// running on another worker now: then its answer, once it has one. A
// script flagged `budget` (a search) isn't run once the search budget is
// used up: its answer is `budget`.
static void run_one(Proc *p, const char *solver, const char *flags, const char *script, size_t script_len, Buf *result) {
  uint64_t h1, h2;
  memo_hash(flags, script, script_len, &h1, &h2);
  int charged = has_word(flags, "budget");
  pthread_mutex_lock(&memo_lock);
  for (;;) {
    if (memo_find(h1, h2, script_len, result)) {
      pthread_mutex_unlock(&memo_lock);
      solver_log(flags, script_len, h1, 0, result->buf, result->len, "\tagain");
      return;
    }
    if (!running_has(h1, h2, script_len)) break;
    pthread_cond_wait(&running_done, &memo_lock);
  }
  if (charged) {
    int64_t limit = __atomic_load_n(&budget_units, __ATOMIC_SEQ_CST);
    if (limit >= 0 && __atomic_load_n(&budget_used, __ATOMIC_SEQ_CST) >= limit) {
      pthread_mutex_unlock(&memo_lock);
      __atomic_fetch_add(&budget_refused, 1, __ATOMIC_SEQ_CST);
      buf_str(result, "budget\n");
      solver_log(flags, script_len, h1, 0, result->buf, result->len, "\tbudget");
      return;
    }
  }
  running[running_count++] = (Running){h1, h2, script_len};
  pthread_mutex_unlock(&memo_lock);
  run_fresh(p, solver, flags, script, script_len, h1, h2, result);
  if (charged) __atomic_fetch_add(&budget_used, units_used(flags, result), __ATOMIC_SEQ_CST);
  pthread_mutex_lock(&memo_lock);
  for (int i = 0; i < running_count; i++) {
    if (running[i].h1 == h1 && running[i].h2 == h2 && running[i].len == script_len) {
      running[i] = running[--running_count];
      break;
    }
  }
  pthread_cond_broadcast(&running_done);
  pthread_mutex_unlock(&memo_lock);
}

// The jobs given to the kept processes, by number, in a queue served by
// worker threads (one per core, each with its own process, started on the
// first job): `solver_pool_run` waits for its own jobs (from any number of
// threads at once: the compiler's workers each encoding a function ask as
// they go); `solver_pool_prefetch` queues jobs whose answers are only kept,
// for a later run to find.
typedef struct {
  char *solver, *flags, *script;
  size_t len;
  Buf result;
  int state;   // 0 queued, 1 running, 2 answered, 3 taken
  int async;   // 0: run (waited for); 2: prefetched (only its answer kept)
} Job;

static Job *jobs;
static long jobs_count, jobs_cap, jobs_next;
static int workers_started;
static pthread_mutex_t queue_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t queue_more = PTHREAD_COND_INITIALIZER;
static pthread_cond_t queue_done = PTHREAD_COND_INITIALIZER;
static char proc_solver[POOL_MAX][4096];

static void job_free(Job *j) {
  free(j->solver);
  free(j->flags);
  free(j->script);
  free(j->result.buf);
  j->solver = j->flags = j->script = NULL;
  j->result = (Buf){0};
  j->state = 3;
}

// The next job to run (-1: none), under `queue_lock`: the first queued,
// a prefetched one only when no other waits.
static long next_job(void) {
  while (jobs_next < jobs_count && jobs[jobs_next].state != 0) jobs_next++;
  long later = -1;
  for (long k = jobs_next; k < jobs_count; k++) {
    if (jobs[k].state != 0) continue;
    if (jobs[k].async != 2) return k;
    if (later < 0) later = k;
  }
  return later;
}

static void *worker(void *arg) {
  long i = (long)arg;
  Proc *p = &procs[i];
  pthread_mutex_lock(&queue_lock);
  for (;;) {
    long k;
    while ((k = next_job()) < 0) pthread_cond_wait(&queue_more, &queue_lock);
    jobs[k].state = 1;
    // (the job's fields stay where they are: `jobs` grows only by
    // `realloc` under the lock, so a copy of them is taken here)
    char *solver = jobs[k].solver, *flags = jobs[k].flags, *script = jobs[k].script;
    size_t len = jobs[k].len;
    pthread_mutex_unlock(&queue_lock);
    if (strcmp(proc_solver[i], solver) != 0) {
      proc_stop(p);
      snprintf(proc_solver[i], sizeof proc_solver[i], "%s", solver);
    }
    Buf result = {0};
    run_one(p, solver, flags, script, len, &result);
    pthread_mutex_lock(&queue_lock);
    jobs[k].result = result;
    jobs[k].state = 2;
    free(jobs[k].script);
    jobs[k].script = NULL;
    if (jobs[k].async == 2) job_free(&jobs[k]);
    pthread_cond_broadcast(&queue_done);
  }
  return NULL;
}

// `jobs` (each `FLAGS 0x1f SCRIPT`, separated by 0x1e) queued, under
// `queue_lock`; the number of the first, the others after it.
static long queue_jobs(const char *solver, const char *text, int async) {
  if (!workers_started) {
    long cores = sysconf(_SC_NPROCESSORS_ONLN);
    int threads = cores > 0 ? (int)cores : 4;
    if (threads > POOL_MAX) threads = POOL_MAX;
    // (a script written to a solver that has exited must not end this process)
    signal(SIGPIPE, SIG_IGN);
    for (long i = 0; i < threads; i++) {
      pthread_t t;
      pthread_attr_t a;
      pthread_attr_init(&a);
      pthread_attr_setdetachstate(&a, PTHREAD_CREATE_DETACHED);
      pthread_create(&t, &a, worker, (void *)i);
      pthread_attr_destroy(&a);
    }
    workers_started = 1;
  }
  long first = jobs_count;
  const char *c = text;
  if (!*c) return first;
  for (;;) {
    const char *end = strchr(c, 0x1e);
    size_t n = end ? (size_t)(end - c) : strlen(c);
    const char *sep = memchr(c, 0x1f, n);
    if (jobs_count == jobs_cap) {
      jobs_cap = jobs_cap ? 2 * jobs_cap : 256;
      jobs = realloc(jobs, (size_t)jobs_cap * sizeof(Job));
    }
    Job *j = &jobs[jobs_count++];
    memset(j, 0, sizeof *j);
    j->solver = strdup(solver);
    if (sep) {
      j->flags = strndup(c, (size_t)(sep - c));
      j->script = strndup(sep + 1, n - (size_t)(sep - c) - 1);
    } else {
      j->flags = strdup("");
      j->script = strndup(c, n);
    }
    j->len = strlen(j->script);
    j->async = async;
    if (!end) break;
    c = end + 1;
  }
  pthread_cond_broadcast(&queue_more);
  return first;
}


// The answers to `jobs` (each `FLAGS 0x1f SCRIPT`, separated by 0x1e), in
// order and separated by 0x1e, from as many of the kept processes at once
// as there are cores.
const char *solver_pool_run(const char *solver, const char *text) {
  pthread_mutex_lock(&queue_lock);
  free(last_result);
  last_result = NULL;
  long first = queue_jobs(solver, text, 0);
  long last = jobs_count;
  Buf all = {0};
  buf_str(&all, "");
  for (long k = first; k < last; k++) {
    while (jobs[k].state != 2) pthread_cond_wait(&queue_done, &queue_lock);
    if (k > first) buf_add(&all, "\x1e", 1);
    if (jobs[k].result.len) buf_add(&all, jobs[k].result.buf, jobs[k].result.len);
    job_free(&jobs[k]);
  }
  last_result = all.buf;
  pthread_mutex_unlock(&queue_lock);
  return last_result;
}

// Whether the program `solver` (looked up in PATH unless it holds a `/`)
// is a script (`#!`): a stand-in, which may answer only once its input
// ends, as the tests' do (one claims to be Z3).
static int is_script(const char *solver) {
  char path[4096];
  if (strchr(solver, '/')) {
    snprintf(path, sizeof path, "%s", solver);
  } else {
    const char *dirs = getenv("PATH");
    path[0] = 0;
    while (dirs && *dirs) {
      const char *end = strchr(dirs, ':');
      size_t n = end ? (size_t)(end - dirs) : strlen(dirs);
      snprintf(path, sizeof path, "%.*s/%s", (int)n, dirs, solver);
      if (access(path, X_OK) == 0) break;
      path[0] = 0;
      dirs = end ? end + 1 : NULL;
    }
  }
  FILE *f = path[0] ? fopen(path, "rb") : NULL;
  if (!f) return 1;
  char head[2] = {0, 0};
  size_t got = fread(head, 1, 2, f);
  fclose(f);
  return got == 2 && head[0] == '#' && head[1] == '!';
}

// What `solver --version` says (its first line), asked once per solver: ""
// for a script standing in for one (the tests'), which isn't asked.
static pthread_mutex_t version_lock = PTHREAD_MUTEX_INITIALIZER;
static char version_solver[4096], version_text[512];
static int version_known;

const char *solver_version(const char *solver) {
  pthread_mutex_lock(&version_lock);
  if (version_known && strcmp(version_solver, solver) == 0) {
    pthread_mutex_unlock(&version_lock);
    return version_text;
  }
  char text[512];
  size_t n = 0;
  text[0] = 0;
  int out[2];
  if (!is_script(solver) && pipe(out) == 0) {
    pid_t pid = fork();
    if (pid == 0) {
      dup2(out[1], 1);
      int devnull = open("/dev/null", O_RDWR);
      if (devnull >= 0) {
        dup2(devnull, 0);
        dup2(devnull, 2);
      }
      for (int fd = 3; fd < 1024; fd++) close(fd);
      execl(solver, solver, "--version", (char *)NULL);
      execlp(solver, solver, "--version", (char *)NULL);
      _exit(127);
    }
    close(out[1]);
    if (pid > 0) {
      ssize_t got;
      while (n < sizeof text - 1 && (got = read(out[0], text + n, sizeof text - 1 - n)) > 0) n += (size_t)got;
      text[n] = 0;
      waitpid(pid, NULL, 0);
    }
    close(out[0]);
  }
  char *nl = strchr(text, '\n');
  if (nl) *nl = 0;
  snprintf(version_solver, sizeof version_solver, "%s", solver);
  snprintf(version_text, sizeof version_text, "%s", text);
  version_known = 1;
  pthread_mutex_unlock(&version_lock);
  return version_text;
}

// Whether `solver` is kept running: Z3 (its `--version` says so), which
// answers each command as it reads it, and not a script standing in for it.
int32_t solver_pool_usable(const char *solver) {
  const char *off = getenv("STRATA_SOLVER_POOL");
  if ((off && strcmp(off, "0") == 0) || pool_broken) return 0;
  return strstr(solver_version(solver), "Z3 version") != NULL;
}

// `jobs` (as `solver_pool_run` takes them) queued to run when nothing else
// waits, their answers only kept (the memo): a later run of the same job
// gets the answer, or waits for it if it is running.
int32_t solver_pool_prefetch(const char *solver, const char *text) {
  pthread_mutex_lock(&queue_lock);
  queue_jobs(solver, text, 2);
  pthread_mutex_unlock(&queue_lock);
  return 0;
}

// Counters the compiler's workers take work by (`smt.counted_map`): each
// worker asks for the next element as it finishes one, so a long one
// doesn't hold up those shared out after it.
static int64_t work_counters[1024];
static int64_t work_counters_used;

int64_t work_counter(void) {
  int64_t id = __atomic_fetch_add(&work_counters_used, 1, __ATOMIC_SEQ_CST) % 1024;
  __atomic_store_n(&work_counters[id], 0, __ATOMIC_SEQ_CST);
  return id;
}

int64_t work_take(int64_t id) {
  return __atomic_fetch_add(&work_counters[id], 1, __ATOMIC_SEQ_CST);
}

// The current time, in nanoseconds since the epoch (for strata.log's
// entries `verify` writes).
int64_t solver_clock_ns(void) {
  struct timespec ts;
  clock_gettime(CLOCK_REALTIME, &ts);
  return (int64_t)ts.tv_sec * 1000000000 + ts.tv_nsec;
}
