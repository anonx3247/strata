// What the `strata` command (compiler/driver, compiler/main) asks of the
// operating system that std doesn't give: the current folder changed (the
// compiler reads core/ and std/ from its home), an environment
// variable set for the commands that read it (`STRATA_HOME`), the
// process id (a scratch folder of its own), and a pager for what it
// prints to read (`driver_pager`).

#include <errno.h>
#include <spawn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
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

// Whether compiler/codegen's output `ir` has a line starting with `error`
// (`codegen_errors`): 1, else 0. In C: the program's IR is tens of
// megabytes, searched once a build.
int32_t driver_has_error_line(const char *ir) {
  return strncmp(ir, "error", 5) == 0 || strstr(ir, "\nerror") != NULL;
}

static pid_t pager_pid = -1;

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
  dup2(fds[1], 1);
  close(fds[1]);
  strata_host_paged();
  atexit(pager_done);
  return 0;
}

// ---- `--timings` (STRATA_TIMINGS=1): the time each phase takes ----
//
// compiler/timings marks where a phase begins and ends (`timings_begin`,
// `timings_end`, nested: a phase inside another is taken out of the
// other's own time), the LLVM glue its own (c/toolchain.cpp), and
// `timings_report` writes them on stderr when the command ends: each
// phase's own time (without the phases inside it), its time in all (an
// inner run of the same phase counted once) and how many times it began.
// Off, a mark costs a test.

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
