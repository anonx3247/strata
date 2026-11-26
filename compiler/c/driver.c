// What the `strata` command asks of the operating system that std doesn't
// give: changing the current folder (the compiler reads core/ and std/ from
// its home), setting environment variables (`STRATA_HOME`), the process id,
// a pager (`driver_pager`), and `--timings`.

#include <errno.h>
#include <spawn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <poll.h>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

extern char **environ;

// Standard output counts as a terminal from now on (runtime.c): it goes to
// a pager that shows it on one, so colors stay on.
extern void strata_host_paged(void);

// The current folder changed to `path`: 0, else 1.
int32_t driver_chdir(const char *path) { return chdir(path) == 0 ? 0 : 1; }

// The environment variable `name` set to `value` (for this process and
// what it starts): 0, else 1.
int32_t driver_setenv(const char *name, const char *value) { return setenv(name, value, 1) == 0 ? 0 : 1; }

// This process's id.
int64_t driver_pid(void) { return (int64_t)getpid(); }

// Whether stream `fd` (0, 1 or 2) is a terminal: 1, else 0. Standard
// output piped away (an agent's run) means no one is at the terminal to
// use an editor `strata edit` would open.
int32_t driver_terminal(int32_t fd) { return isatty(fd) ? 1 : 0; }

// Whether standard input gives nothing to read: 1 when it is a terminal,
// or neither a file, a pipe nor /dev/null (a socket or a device, as an
// agent's shell may leave there, open and silent) and has nothing to read
// within 100 ms; else 0 (reading it ends, or gives something).
int32_t driver_stdin_idle(void) {
  struct stat st;
  if (fstat(0, &st) != 0) return 1;
  if (S_ISREG(st.st_mode) || S_ISFIFO(st.st_mode)) return 0;
  if (isatty(0)) return 1;
  struct stat null_st;
  if (stat("/dev/null", &null_st) == 0 && S_ISCHR(st.st_mode) && st.st_rdev == null_st.st_rdev) return 0;
  struct pollfd p = {.fd = 0, .events = POLLIN};
  int n = poll(&p, 1, 100);
  return n > 0 && (p.revents & (POLLIN | POLLHUP)) && !(p.revents & POLLNVAL) ? 0 : 1;
}

// Whether compiler/codegen's output `ir` has a line starting with `error`
// (`codegen_errors`): 1, else 0. In C: the program's IR is tens of
// megabytes, searched once a build.
int32_t driver_has_error_line(const char *ir) {
  return strncmp(ir, "error", 5) == 0 || strstr(ir, "\nerror") != NULL;
}

static pid_t pager_pid = -1;

// The terminal's width when standard output went to a pager, else 0.
static int64_t paged_columns = 0;

static int64_t columns_of(int fd) {
  struct winsize w;
  return isatty(fd) && ioctl(fd, TIOCGWINSZ, &w) == 0 ? (int64_t)w.ws_col : 0;
}

// The width in columns of the terminal standard output shows on, through a
// pager or not. 0 when it is not a terminal.
int64_t driver_columns(void) { return paged_columns > 0 ? paged_columns : columns_of(1); }

// Whether standard output goes to a pager this process started: 1, else 0.
int32_t driver_paged(void) { return pager_pid > 0 ? 1 : 0; }

// The pager's width, for a request the daemon serves (c/daemon.c): the
// client's pager, 0 for none.
void driver_set_columns(int64_t columns) { paged_columns = columns; }

// At exit: the pager gets the end of its input, and is waited for (it
// keeps the terminal until it is quit).
static void pager_done(void) {
  fflush(stdout);
  close(1);
  int status;
  while (waitpid(pager_pid, &status, 0) < 0 && errno == EINTR) {
  }
}

// Standard output sent to a pager, as git does it: `cmd` run by the shell
// with its standard input a pipe, and this process's standard output that
// pipe, until it exits (then the pager is waited for). Only when standard
// output is a terminal. 0 when the pager runs, else 1 (output unchanged).
int32_t driver_pager(const char *cmd) {
  if (!isatty(1) || cmd == NULL || *cmd == 0) return 1;
  int fds[2];
  if (pipe(fds) != 0) return 1;
  posix_spawn_file_actions_t acts;
  posix_spawn_file_actions_init(&acts);
  posix_spawn_file_actions_adddup2(&acts, fds[0], 0);
  posix_spawn_file_actions_addclose(&acts, fds[0]);
  posix_spawn_file_actions_addclose(&acts, fds[1]);
  char *argv[] = {"sh", "-c", (char *)cmd, NULL};
  int failed = posix_spawn(&pager_pid, "/bin/sh", &acts, NULL, argv, environ);
  posix_spawn_file_actions_destroy(&acts);
  close(fds[0]);
  if (failed) {
    close(fds[1]);
    return 1;
  }
  fflush(stdout);
  paged_columns = columns_of(1);
  dup2(fds[1], 1);
  close(fds[1]);
  strata_host_paged();
  atexit(pager_done);
  return 0;
}

// ---- `--timings` (STRATA_TIMINGS=1): the time each phase takes ----
//
// compiler/timings and the LLVM glue (c/toolchain.c) mark where a phase
// begins and ends (`timings_begin`, `timings_end`). Phases nest: a phase
// inside another is taken out of the other's own time. `timings_report`
// writes on stderr, when the command ends, each phase's own time, its time
// in all (an inner run of the same phase counted once) and how many times
// it began. When off, a mark costs a test.

#include <stdio.h>
#include <string.h>
#include <time.h>

static int timings_on = -1;
static double timings_start;

typedef struct {
  char *name;
  double own, total;
  long count;
  int depth;
} Phase;
static Phase phases[64];
static int phase_count;

typedef struct {
  int phase;
  double start, inner;
} Running;
static Running running[512];
static int running_count;

static double timings_now(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return (double)t.tv_sec + (double)t.tv_nsec * 1e-9;
}

// Whether phases are timed: `--timings` (`timings_enable`), or
// STRATA_TIMINGS set and not `0`.
int32_t timings_enabled(void) {
  if (timings_on < 0) {
    const char *p = getenv("STRATA_TIMINGS");
    timings_on = p && *p && strcmp(p, "0") != 0;
    if (timings_on) timings_start = timings_now();
  }
  return timings_on;
}

// Nothing timed yet, and whether to time read again from the environment
// (each request a daemon serves, c/daemon.c, starts so).
void timings_reset(void) {
  for (int i = 0; i < phase_count; i++) free(phases[i].name);
  phase_count = 0;
  running_count = 0;
  timings_on = -1;
}

// Phases timed from now on (`--timings`).
int32_t timings_enable(void) {
  if (timings_enabled() <= 0) {
    timings_on = 1;
    timings_start = timings_now();
  }
  return 0;
}

int32_t timings_begin(const char *name) {
  if (!timings_enabled()) return 0;
  int i = 0;
  while (i < phase_count && strcmp(phases[i].name, name) != 0) i++;
  if (i == phase_count) {
    if (phase_count == 64) return 0;
    phases[i].name = strdup(name);
    phase_count++;
  }
  if (running_count == 512) return 0;
  phases[i].depth++;
  phases[i].count++;
  running[running_count++] = (Running){i, timings_now(), 0};
  return 0;
}

int32_t timings_end(const char *name) {
  if (!timings_enabled()) return 0;
  // (the innermost run of `name`: what began inside it and wasn't ended
  // ends with it)
  int k = running_count - 1;
  while (k >= 0 && strcmp(phases[running[k].phase].name, name) != 0) k--;
  if (k < 0) return 0;
  double now = timings_now();
  while (running_count > k) {
    Running r = running[--running_count];
    Phase *p = &phases[r.phase];
    double t = now - r.start;
    p->own += t - r.inner;
    if (--p->depth == 0) p->total += t;
    if (running_count > 0) running[running_count - 1].inner += t;
  }
  return 0;
}

// The phases' times on stderr, the largest own time first (nothing when
// they weren't timed).
int32_t timings_report(void) {
  if (timings_enabled() <= 0) return 0;
  double wall = timings_now() - timings_start;
  int order[64];
  for (int i = 0; i < phase_count; i++) order[i] = i;
  for (int i = 1; i < phase_count; i++)
    for (int j = i; j > 0 && phases[order[j]].own > phases[order[j - 1]].own; j--) {
      int t = order[j];
      order[j] = order[j - 1];
      order[j - 1] = t;
    }
  double timed = 0;
  fprintf(stderr, "timings (s): %8s %8s %7s  phase\n", "own", "in all", "times");
  for (int i = 0; i < phase_count; i++) {
    Phase *p = &phases[order[i]];
    timed += p->own;
    fprintf(stderr, "             %8.3f %8.3f %7ld  %s\n", p->own, p->total, p->count, p->name);
  }
  fprintf(stderr, "             %8.3f %8s %7s  (the rest)\n", wall - timed, "", "");
  fprintf(stderr, "             %8.3f %8s %7s  in all\n", wall, "", "");
  return 0;
}

// How many threads the compiler's parallel work may use: STRATA_JOBS when
// set (at least 1), else the cores the machine has online.
int64_t driver_jobs(void) {
  const char *j = getenv("STRATA_JOBS");
  if (j && *j) {
    long n = atol(j);
    return n < 1 ? 1 : n;
  }
  long n = sysconf(_SC_NPROCESSORS_ONLN);
  return n < 1 ? 1 : n;
}

// A 60-bit FNV-1a hash of a string (codegen names string constants by it).
int64_t text_hash(const char *s) {
  uint64_t h = 14695981039346656037ull;
  for (; *s; s++) h = (h ^ (unsigned char)*s) * 1099511628211ull;
  return (int64_t)(h >> 4);
}

// While `on`, objects made before then that are shared while tasks run are
// kept for good (core.rt's strata_memory_keep_shared): the compiler's
// workers all read the same tables, whose counts would otherwise change
// atomically from every core. What the workers make is counted as usual.
void strata_memory_keep_shared(_Bool on);
int32_t driver_keep_shared(int32_t on) {
  strata_memory_keep_shared(on != 0);
  return 0;
}
