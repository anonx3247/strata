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
