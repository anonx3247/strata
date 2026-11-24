// What the `strata` command (compiler/driver, compiler/main) asks of the
// operating system that std doesn't give: this executable's own path (to
// find the repository's core/ and std/ beside it), the current folder
// changed (the compiler reads core/ and std/ from its home), an environment
// variable set for the commands that read it (`STRATA_HOME`), and the
// process id (a scratch folder of its own).

#include <stdint.h>
#include <stdlib.h>
#include <unistd.h>
#ifdef __APPLE__
#include <mach-o/dyld.h>
#endif

static char self_path[4096];

// This executable's path, its links resolved; "" when it can't be told.
const char *driver_self_path(void) {
  char path[4096];
#ifdef __APPLE__
  uint32_t n = sizeof path;
  if (_NSGetExecutablePath(path, &n) != 0) return "";
#else
  ssize_t n = readlink("/proc/self/exe", path, sizeof path - 1);
  if (n <= 0) return "";
  path[n] = 0;
#endif
  if (!realpath(path, self_path)) return "";
  return self_path;
}

// The current folder changed to `path`: 0, else 1.
int32_t driver_chdir(const char *path) { return chdir(path) == 0 ? 0 : 1; }

// The environment variable `name` set to `value` (for this process and
// what it starts): 0, else 1.
int32_t driver_setenv(const char *name, const char *value) { return setenv(name, value, 1) == 0 ? 0 : 1; }

// This process's id.
int64_t driver_pid(void) { return (int64_t)getpid(); }
