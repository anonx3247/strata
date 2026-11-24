// Long-lived solver processes for compiler/smt: Z3 started once per core
// (`z3 -in`) and given one script after another, instead of a process per
// script (starting Z3 costs ~8 ms, more than most queries take).
//
// A script runs after `(reset)` (assertions and declarations gone) with the
// limits it was given set again (`:timeout`, `:rlimit`: they outlive a
// reset), then `(echo "@@strata-done")` marks the end of its answers.
// Z3's resource limit counts per `check-sat`, so a script gets the answers
// it gets in a process of its own (the verdict cache relies on that); the
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

typedef struct {
  pid_t pid;      // 0: not running
  int in, out;    // its standard input and output
  long lines;     // lines written to it so far
} Proc;

typedef struct {
  char *buf;
  size_t len, cap;
} Buf;

static Proc procs[POOL_MAX];
static char pool_solver[4096];
static pthread_mutex_t pool_lock = PTHREAD_MUTEX_INITIALIZER;
static char *last_result;
// (a solver started that didn't answer a command as it read it: scripts go
// to a process each from then on)
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
}

// At exit: the processes told their input has ended, and waited for (their
// time is this process's children's, as a process per script's was).
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

// The answer kept for a script, copied into `result`; 0 when there is none.
static int memo_get(uint64_t h1, uint64_t h2, size_t n, Buf *result) {
  int found = 0;
  pthread_mutex_lock(&memo_lock);
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
  pthread_mutex_unlock(&memo_lock);
  return found;
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

// STRATA_SOLVER_LOG=FILE: each script's time, flags, size, hash and first
// answer, a line each (`again`: answered before, not asked).
static void solver_log(const char *flags, size_t n, uint64_t h, double secs, const char *out, size_t len, const char *note) {
  const char *log = getenv("STRATA_SOLVER_LOG");
  if (!log || !*log) return;
  FILE *lf = fopen(log, "a");
  if (!lf) return;
  size_t first = 0;
  while (first < len && out[first] != '\n') first++;
  fprintf(lf, "%.4f\t%s\t%zu\t%016llx\t%.*s%s\n", secs, flags, n, (unsigned long long)h, (int)(first < 60 ? first : 60), out ? out : "", note);
  fclose(lf);
}

// One script's answers from process `p` (started when it isn't running),
// with its flags (`-t:MS`, `-T:S`, `rlimit=N`): what the solver wrote
// before the end mark, or, when it died or ran past `-T`, what it wrote.
static void run_one(Proc *p, const char *solver, const char *flags, const char *script, size_t script_len, Buf *result) {
  uint64_t h1, h2;
  memo_hash(flags, script, script_len, &h1, &h2);
  if (memo_get(h1, h2, script_len, result)) {
    solver_log(flags, script_len, h1, 0, result->buf, result->len, "\tagain");
    return;
  }
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
  if (p->pid == 0 && !proc_start(p, solver)) return;
  char head[256];
  snprintf(head, sizeof head, "(reset)\n(set-option :timeout %ld)\n(set-option :rlimit %ld)\n", timeout_ms, rlimit);
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
  if (out.len) fix_lines(result, out.buf, out.len, base);
  solver_log(flags, script_len, h1, now_s() - started, out.buf, out.len, "");
  if (!done) {
    proc_stop(p);
    if (dead == 2) buf_str(result, "timeout\n");
  } else {
    memo_put(h1, h2, script_len, result->buf ? result->buf : "");
  }
  free(in.buf);
  free(out.buf);
}

typedef struct {
  const char *solver;
  char **flags, **scripts;
  size_t *lens;
  Buf *results;
  int jobs, next;
  pthread_mutex_t lock;
} Work;

typedef struct {
  Work *w;
  Proc *p;
} Worker;

static void *worker(void *arg) {
  Worker *k = arg;
  Work *w = k->w;
  for (;;) {
    pthread_mutex_lock(&w->lock);
    int i = w->next++;
    pthread_mutex_unlock(&w->lock);
    if (i >= w->jobs) break;
    run_one(k->p, w->solver, w->flags[i], w->scripts[i], w->lens[i], &w->results[i]);
  }
  return NULL;
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

// Whether `solver` is kept running: Z3 (its `--version` says so), which
// answers each command as it reads it, and not a script standing in for it.
int32_t solver_pool_usable(const char *solver) {
  static char known[4096];
  static int answer = -1;
  const char *off = getenv("STRATA_SOLVER_POOL");
  if ((off && strcmp(off, "0") == 0) || pool_broken) return 0;
  pthread_mutex_lock(&pool_lock);
  if (answer >= 0 && strcmp(known, solver) == 0) {
    int a = answer;
    pthread_mutex_unlock(&pool_lock);
    return a;
  }
  int a = 0;
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
      char text[512];
      size_t n = 0;
      ssize_t got;
      while (n < sizeof text - 1 && (got = read(out[0], text + n, sizeof text - 1 - n)) > 0) n += (size_t)got;
      text[n] = 0;
      waitpid(pid, NULL, 0);
      a = strstr(text, "Z3 version") != NULL;
    }
    close(out[0]);
  }
  snprintf(known, sizeof known, "%s", solver);
  answer = a;
  pthread_mutex_unlock(&pool_lock);
  return a;
}

// The answers to `jobs` (each `FLAGS 0x1f SCRIPT`, separated by 0x1e), in
// order and separated by 0x1e, from as many of the kept processes at once
// as there are cores.
const char *solver_pool_run(const char *solver, const char *jobs) {
  pthread_mutex_lock(&pool_lock);
  if (strcmp(pool_solver, solver) != 0) {
    for (int i = 0; i < POOL_MAX; i++) proc_stop(&procs[i]);
    snprintf(pool_solver, sizeof pool_solver, "%s", solver);
  }
  free(last_result);
  last_result = NULL;
  // the jobs, cut apart in a copy
  char *copy = strdup(jobs);
  int n = 0;
  for (char *c = copy; *c; c++)
    if (*c == 0x1e) n++;
  n += copy[0] != 0;
  Work w = {solver, calloc(n + 1, sizeof(char *)), calloc(n + 1, sizeof(char *)), calloc(n + 1, sizeof(size_t)), calloc(n + 1, sizeof(Buf)), n, 0, PTHREAD_MUTEX_INITIALIZER};
  char *c = copy;
  for (int i = 0; i < n; i++) {
    char *end = strchr(c, 0x1e);
    if (end) *end = 0;
    char *sep = strchr(c, 0x1f);
    if (sep) {
      *sep = 0;
      w.flags[i] = c;
      w.scripts[i] = sep + 1;
    } else {
      w.flags[i] = "";
      w.scripts[i] = c;
    }
    w.lens[i] = strlen(w.scripts[i]);
    c = end ? end + 1 : c + strlen(c);
  }
  long cores = sysconf(_SC_NPROCESSORS_ONLN);
  int threads = cores > 0 ? (int)cores : 4;
  if (threads > POOL_MAX) threads = POOL_MAX;
  if (threads > n) threads = n;
  // (a script written to a solver that has exited must not end this process)
  signal(SIGPIPE, SIG_IGN);
  Worker ks[POOL_MAX];
  pthread_t ts[POOL_MAX];
  for (int i = 0; i < threads; i++) {
    ks[i] = (Worker){&w, &procs[i]};
    if (i > 0) pthread_create(&ts[i], NULL, worker, &ks[i]);
  }
  if (threads > 0) worker(&ks[0]);
  for (int i = 1; i < threads; i++) pthread_join(ts[i], NULL);
  Buf all = {0};
  buf_str(&all, "");
  for (int i = 0; i < n; i++) {
    if (i > 0) buf_add(&all, "\x1e", 1);
    if (w.results[i].len) buf_add(&all, w.results[i].buf, w.results[i].len);
    free(w.results[i].buf);
  }
  free(w.flags);
  free(w.scripts);
  free(w.lens);
  free(w.results);
  free(copy);
  last_result = all.buf;
  pthread_mutex_unlock(&pool_lock);
  return last_result;
}
