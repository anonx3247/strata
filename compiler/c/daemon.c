// What compiler/daemon asks of the system: a Unix socket a long-running
// `strata` listens on, and the client's side of a request.
//
// A request is a command line run by the daemon as if the client's process
// ran it: the client sends its arguments, its current folder, its
// environment and its standard input, output and error (the descriptors
// themselves, passed over the socket), and the daemon runs the command
// with those as its own, then sends back the exit code. Whatever the
// command writes goes straight to the client's terminal, pipe or file, and
// a terminal is still a terminal (colors, widths), so the output is the
// same bytes as without a daemon. One request runs at a time (the
// descriptors, the folder and the environment are the process's); others
// wait in the socket's queue.
//
// A message is a kind byte, a 4-byte length and that many bytes: 'R' runs
// a command (with the three descriptors), 'I' asks what the daemon holds,
// 'S' stops it. The daemon answers 'R' with its process id (8 bytes, so
// the client can pass on an interrupt), then the exit code (4 bytes: -1
// when it won't serve the request, which the client then runs itself).

#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <signal.h>
#include <spawn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#ifdef __APPLE__
#include <mach/mach.h>
#endif

extern char **environ;

// (runtime.c: the arguments `process.args()` gives, and whether standard
// output counts as a terminal's pager; whether the client's output closed)
void strata_host_serve(int argc, char **argv, _Bool paged);
_Bool strata_host_closed(void);
// (c/driver.c: the pager's terminal width, the phase timers; c/jit.c:
// `--profile` and the JIT's optimization level, read again from the
// environment)
int64_t driver_columns(void);
int32_t driver_paged(void);
void driver_set_columns(int64_t columns);
void timings_reset(void);
void toolchain_reset(void);

// ---- messages ----

static int write_all(int fd, const void *p, size_t n) {
  const char *c = p;
  while (n > 0) {
    ssize_t k = write(fd, c, n);
    if (k < 0 && errno == EINTR) continue;
    if (k <= 0) return -1;
    c += k;
    n -= (size_t)k;
  }
  return 0;
}

static int read_all(int fd, void *p, size_t n) {
  char *c = p;
  while (n > 0) {
    ssize_t k = read(fd, c, n);
    if (k < 0 && errno == EINTR) continue;
    if (k <= 0) return -1;
    c += k;
    n -= (size_t)k;
  }
  return 0;
}

// The kind and length, with the descriptors `fds` (none when `nfds` is 0)
// passed alongside; then the bytes.
static int send_message(int s, char kind, const char *body, uint32_t len, const int *fds, int nfds) {
  char head[5];
  head[0] = kind;
  memcpy(head + 1, &len, 4);
  struct iovec io = {.iov_base = head, .iov_len = 5};
  struct msghdr m;
  memset(&m, 0, sizeof m);
  m.msg_iov = &io;
  m.msg_iovlen = 1;
  char ctl[CMSG_SPACE(3 * sizeof(int))];
  if (nfds > 0) {
    memset(ctl, 0, sizeof ctl);
    m.msg_control = ctl;
    m.msg_controllen = CMSG_SPACE((size_t)nfds * sizeof(int));
    struct cmsghdr *c = CMSG_FIRSTHDR(&m);
    c->cmsg_level = SOL_SOCKET;
    c->cmsg_type = SCM_RIGHTS;
    c->cmsg_len = CMSG_LEN((size_t)nfds * sizeof(int));
    memcpy(CMSG_DATA(c), fds, (size_t)nfds * sizeof(int));
  }
  ssize_t k;
  do {
    k = sendmsg(s, &m, 0);
  } while (k < 0 && errno == EINTR);
  if (k != 5) return -1;
  return write_all(s, body, len);
}

// A message read: its kind (0 when the connection broke), its bytes
// (malloc'd, NUL-terminated) and up to three descriptors (-1 for none).
static char recv_message(int s, char **body, uint32_t *len, int fds[3]) {
  fds[0] = fds[1] = fds[2] = -1;
  char head[5];
  struct iovec io = {.iov_base = head, .iov_len = 5};
  struct msghdr m;
  memset(&m, 0, sizeof m);
  m.msg_iov = &io;
  m.msg_iovlen = 1;
  char ctl[CMSG_SPACE(3 * sizeof(int))];
  m.msg_control = ctl;
  m.msg_controllen = sizeof ctl;
  ssize_t k;
  do {
    k = recvmsg(s, &m, MSG_WAITALL);
  } while (k < 0 && errno == EINTR);
  if (k != 5) return 0;
  for (struct cmsghdr *c = CMSG_FIRSTHDR(&m); c; c = CMSG_NXTHDR(&m, c)) {
    if (c->cmsg_level == SOL_SOCKET && c->cmsg_type == SCM_RIGHTS) {
      int n = (int)((c->cmsg_len - CMSG_LEN(0)) / sizeof(int));
      for (int i = 0; i < n && i < 3; i++) memcpy(&fds[i], CMSG_DATA(c) + (size_t)i * sizeof(int), sizeof(int));
    }
  }
  memcpy(len, head + 1, 4);
  if (*len > (64u << 20)) return 0;
  *body = malloc((size_t)*len + 1);
  if (!*body || read_all(s, *body, *len) != 0) {
    free(*body);
    *body = NULL;
    return 0;
  }
  (*body)[*len] = 0;
  return head[0];
}

static int socket_at(const char *path, struct sockaddr_un *a) {
  if (strlen(path) >= sizeof a->sun_path) return -1;
  memset(a, 0, sizeof *a);
  a->sun_family = AF_UNIX;
  strcpy(a->sun_path, path);
  int s = socket(AF_UNIX, SOCK_STREAM, 0);
  if (s >= 0) fcntl(s, F_SETFD, FD_CLOEXEC);
  return s;
}

static int connected(const char *path) {
  struct sockaddr_un a;
  int s = socket_at(path, &a);
  if (s < 0) return -1;
  if (connect(s, (struct sockaddr *)&a, sizeof a) != 0) {
    close(s);
    return -1;
  }
  return s;
}

// ---- the client ----

static pid_t serving = 0;

// An interrupt or a hangup of the client stops the command: the daemon is
// stopped with it (a command can't be stopped halfway and leave the
// daemon's state whole); the next command starts without it.
static void pass_on(int sig) {
  if (serving > 0) kill(serving, SIGTERM);
  signal(sig, SIG_DFL);
  raise(sig);
}

// Whether a daemon answers at `path`: 1, else 0. A socket file no daemon
// answers at (one that was stopped by a crash) is removed.
int32_t daemon_running(const char *path) {
  int s = connected(path);
  if (s < 0) {
    struct stat st;
    if (stat(path, &st) == 0 && S_ISSOCK(st.st_mode)) unlink(path);
    return 0;
  }
  close(s);
  return 1;
}

// The command line `request` (compiler/daemon's `encoded`: the arguments)
// run, in this process's folder and environment, by the daemon at `path`, with this
// process's standard streams. Its exit code; -1 when no daemon took it (no
// daemon there, or one that won't serve it), and the client runs it
// itself; 101 when the daemon stopped while running it (it said why on
// standard error, as a compiler that crashes does).
int32_t daemon_forward(const char *path, const char *request) {
  int s = connected(path);
  if (s < 0) return -1;
  // (what this process writes before is written first)
  fflush(stdout);
  fflush(stderr);
  // (the header, the arguments, then the folder and the environment)
  char cwd[PATH_MAX];
  if (!getcwd(cwd, sizeof cwd)) {
    close(s);
    return -1;
  }
  size_t cap = strlen(request) + strlen(cwd) + 128;
  int envc = 0;
  for (char **e = environ; e && *e; e++, envc++) cap += strlen(*e) + 24;
  char *body = malloc(cap);
  if (!body) {
    close(s);
    return -1;
  }
  int paged = driver_paged();
  size_t k = (size_t)snprintf(body, cap, "%d %lld\n%s", paged, (long long)(paged ? driver_columns() : 0), request);
  k += (size_t)snprintf(body + k, cap - k, "%zu:%s", strlen(cwd), cwd);
  char count[24];
  int cn = snprintf(count, sizeof count, "%d", envc);
  k += (size_t)snprintf(body + k, cap - k, "%d:%s", cn, count);
  for (char **e = environ; e && *e; e++) k += (size_t)snprintf(body + k, cap - k, "%zu:%s", strlen(*e), *e);
  int fds[3] = {0, 1, 2};
  int failed = send_message(s, 'R', body, (uint32_t)k, fds, 3);
  free(body);
  int64_t pid = 0;
  if (failed || read_all(s, &pid, 8) != 0) {
    close(s);
    return -1;
  }
  serving = (pid_t)pid;
  signal(SIGINT, pass_on);
  signal(SIGTERM, pass_on);
  signal(SIGHUP, pass_on);
  int32_t code = 0;
  int got = read_all(s, &code, 4);
  serving = 0;
  signal(SIGINT, SIG_DFL);
  signal(SIGTERM, SIG_DFL);
  signal(SIGHUP, SIG_DFL);
  close(s);
  return got == 0 ? code : 101;
}

// What the daemon at `path` answers to `kind` ('I': what it holds, 'S':
// stop): its text, "" when none answers.
const char *daemon_ask(const char *path, const char *kind) {
  static char *answer;
  free(answer);
  answer = NULL;
  int s = connected(path);
  if (s < 0) return "";
  int none[3];
  char *body = NULL;
  uint32_t len = 0;
  if (send_message(s, kind[0], "", 0, none, 0) != 0 || recv_message(s, &body, &len, none) == 0) {
    close(s);
    return "";
  }
  close(s);
  answer = body;
  return answer;
}

// The daemon started for `root`, listening at `path`: this executable run
// as `strata daemon serve`, in a session of its own (no terminal: it
// outlives the command that started it), its output to `log`. Waits until
// it answers, up to 10 s. 0 when it does, else 1.
int32_t daemon_start(const char *exe, const char *path, const char *root, const char *log) {
  posix_spawn_file_actions_t acts;
  posix_spawn_file_actions_init(&acts);
  posix_spawn_file_actions_addopen(&acts, 0, "/dev/null", O_RDONLY, 0);
  posix_spawn_file_actions_addopen(&acts, 1, log, O_WRONLY | O_CREAT | O_APPEND, 0600);
  posix_spawn_file_actions_adddup2(&acts, 1, 2);
  posix_spawnattr_t at;
  posix_spawnattr_init(&at);
#ifdef POSIX_SPAWN_SETSID
  posix_spawnattr_setflags(&at, POSIX_SPAWN_SETSID);
#endif
  char *argv[] = {(char *)exe, "daemon", "serve", "--socket", (char *)path, "--root", (char *)root, NULL};
  pid_t pid;
  int e = posix_spawn(&pid, exe, &acts, &at, argv, environ);
  posix_spawn_file_actions_destroy(&acts);
  posix_spawnattr_destroy(&at);
  if (e != 0) return 1;
  for (int i = 0; i < 1000; i++) {
    int s = connected(path);
    if (s >= 0) {
      close(s);
      return 0;
    }
    // (it stopped: a mistake it wrote in the log)
    int status;
    if (waitpid(pid, &status, WNOHANG) == pid) return 1;
    usleep(10000);
  }
  return 1;
}

// What tells a file's versions apart without reading it: its size and
// modification time (`SIZE:SECONDS.NANOSECONDS`), "" when there is none.
const char *daemon_stamp(const char *path) {
  static char out[64];
  struct stat st;
  if (stat(path, &st) != 0) return "";
#ifdef __APPLE__
  snprintf(out, sizeof out, "%lld:%ld.%09ld", (long long)st.st_size, (long)st.st_mtimespec.tv_sec, st.st_mtimespec.tv_nsec);
#else
  snprintf(out, sizeof out, "%lld:%ld.%09ld", (long long)st.st_size, (long)st.st_mtim.tv_sec, st.st_mtim.tv_nsec);
#endif
  return out;
}

// ---- the daemon ----

static int listener = -1;
static char listen_path[PATH_MAX];
static char listen_root[PATH_MAX];
// (the daemon's own standard streams, put back after each request)
static int own_fds[3] = {-1, -1, -1};
// the request running: its connection and the client's streams
static int client = -1;
static int client_fds[3] = {-1, -1, -1};
// (the request's arguments and environment, as `strata_host_serve` and
// `environ` hold them while it runs)
static char **req_argv;
static int req_argc;
static long served_count;
static time_t started_at;

int32_t daemon_done(int32_t code);

static void free_request(void) {
  for (int i = 0; i < req_argc; i++) free(req_argv[i]);
  free(req_argv);
  req_argv = NULL;
  req_argc = 0;
}

// Listening at `path` for requests about the package `root`: 0, else 1
// (another daemon answers there, or the socket can't be made). The socket
// is the user's alone.
int32_t daemon_listen(const char *path, const char *root) {
  if (daemon_running(path)) return 1;
  unlink(path);
  struct sockaddr_un a;
  int s = socket_at(path, &a);
  if (s < 0) return 1;
  mode_t old = umask(077);
  int bad = bind(s, (struct sockaddr *)&a, sizeof a) != 0 || listen(s, 64) != 0;
  umask(old);
  if (bad) {
    close(s);
    return 1;
  }
  listener = s;
  snprintf(listen_path, sizeof listen_path, "%s", path);
  snprintf(listen_root, sizeof listen_root, "%s", root);
  for (int i = 0; i < 3; i++) {
    own_fds[i] = dup(i);
    if (own_fds[i] >= 0) fcntl(own_fds[i], F_SETFD, FD_CLOEXEC);
  }
  started_at = time(NULL);
  signal(SIGPIPE, SIG_IGN);
  return 0;
}

// The socket removed, so that no client finds a daemon that has gone.
int32_t daemon_unlisten(void) {
  if (listener >= 0) {
    close(listener);
    listener = -1;
    unlink(listen_path);
  }
  return 0;
}

// This process's resident memory, in bytes.
int64_t daemon_resident(void) {
#ifdef __APPLE__
  struct mach_task_basic_info info;
  mach_msg_type_number_t n = MACH_TASK_BASIC_INFO_COUNT;
  if (task_info(mach_task_self(), MACH_TASK_BASIC_INFO, (task_info_t)&info, &n) == KERN_SUCCESS)
    return (int64_t)info.resident_size;
  return 0;
#else
  struct rusage u;
  getrusage(RUSAGE_SELF, &u);
  return (int64_t)u.ru_maxrss * 1024;
#endif
}

// A length-prefixed field (`N:bytes`) at `*p`, malloc'd; NULL when the
// text is cut short.
static char *field(const char **p, const char *end) {
  char *colon;
  long n = strtol(*p, &colon, 10);
  if (colon >= end || *colon != ':' || n < 0 || colon + 1 + n > end) return NULL;
  char *out = malloc((size_t)n + 1);
  memcpy(out, colon + 1, (size_t)n);
  out[n] = 0;
  *p = colon + 1 + n;
  return out;
}

// The client's state taken as the process's: its streams, folder,
// environment and arguments. 0, else 1 (a request that can't be read).
static int enter(const char *body, uint32_t len) {
  const char *p = body, *end = body + len;
  int paged = 0;
  long long columns = 0;
  if (sscanf(p, "%d %lld", &paged, &columns) != 2) return 1;
  p = strchr(p, '\n');
  if (!p) return 1;
  p++;
  char *n_args = field(&p, end);
  if (!n_args) return 1;
  int argc = atoi(n_args);
  free(n_args);
  char **argv = calloc((size_t)argc + 1, sizeof(char *));
  for (int i = 0; i < argc; i++)
    if (!(argv[i] = field(&p, end))) return 1;
  char *cwd = field(&p, end);
  char *n_env = field(&p, end);
  if (!cwd || !n_env) return 1;
  int envc = atoi(n_env);
  free(n_env);
  // the environment: exactly the client's
  while (environ && environ[0]) {
    char *eq = strchr(environ[0], '=');
    if (!eq) break;
    char name[1024];
    size_t k = (size_t)(eq - environ[0]);
    if (k >= sizeof name) break;
    memcpy(name, environ[0], k);
    name[k] = 0;
    unsetenv(name);
  }
  for (int i = 0; i < envc; i++) {
    char *kv = field(&p, end);
    if (!kv) return 1;
    char *eq = strchr(kv, '=');
    if (eq) {
      *eq = 0;
      setenv(kv, eq + 1, 1);
    }
    free(kv);
  }
  int bad = chdir(cwd) != 0;
  free(cwd);
  if (bad) return 1;
  fflush(stdout);
  fflush(stderr);
  for (int i = 0; i < 3; i++)
    if (client_fds[i] >= 0) dup2(client_fds[i], i);
#ifdef __APPLE__
  fpurge(stdin);
#endif
  clearerr(stdin);
  clearerr(stdout);
  clearerr(stderr);
  free_request();
  req_argv = argv;
  req_argc = argc;
  strata_host_serve(argc, argv, paged != 0);
  driver_set_columns(paged ? (int64_t)columns : 0);
  timings_reset();
  toolchain_reset();
  return 0;
}

static void drop_client(void) {
  for (int i = 0; i < 3; i++) {
    if (client_fds[i] >= 0) close(client_fds[i]);
    client_fds[i] = -1;
  }
  if (client >= 0) close(client);
  client = -1;
}

static void answer(int s, const char *text) {
  int none[3];
  send_message(s, 'A', text, (uint32_t)strlen(text), none, 0);
}

// The next request, waiting up to `idle_ms` for one: 1 when a command is to
// run (the process now has the client's streams, folder, environment and
// arguments: `daemon_done` gives them back), 0 when none came in that
// time, 2 when one asked the daemon to stop. What the daemon holds ('I')
// is answered here.
int32_t daemon_next(int64_t idle_ms) {
  for (;;) {
    struct pollfd pf = {.fd = listener, .events = POLLIN};
    int r = poll(&pf, 1, idle_ms > INT_MAX ? INT_MAX : (int)idle_ms);
    if (r < 0 && errno == EINTR) continue;
    if (r <= 0) return 0;
    int s = accept(listener, NULL, NULL);
    if (s < 0) continue;
    fcntl(s, F_SETFD, FD_CLOEXEC);
    char *body = NULL;
    uint32_t len = 0;
    int fds[3];
    char kind = recv_message(s, &body, &len, fds);
    if (kind == 'S') {
      answer(s, "stopped");
      close(s);
      free(body);
      return 2;
    }
    if (kind == 'I') {
      char text[PATH_MAX + 256];
      snprintf(text, sizeof text, "pid %d\nroot %s\nsocket %s\nup %lld s\nrequests %ld\nresident %lld MB\n", (int)getpid(), listen_root,
               listen_path, (long long)(time(NULL) - started_at), served_count, (long long)(daemon_resident() >> 20));
      answer(s, text);
      close(s);
      free(body);
      continue;
    }
    if (kind != 'R' || fds[0] < 0 || fds[1] < 0 || fds[2] < 0) {
      for (int i = 0; i < 3; i++)
        if (fds[i] >= 0) close(fds[i]);
      close(s);
      free(body);
      continue;
    }
    client = s;
    for (int i = 0; i < 3; i++) client_fds[i] = fds[i];
    int64_t pid = getpid();
    int bad = write_all(s, &pid, 8) != 0 || enter(body, len) != 0;
    free(body);
    if (bad) {
      int32_t refused = -1;
      write_all(s, &refused, 4);
      daemon_done(-2);
      continue;
    }
    served_count++;
    return 1;
  }
}

// The request ended with exit code `code` (-1: not served, the client runs
// it itself): the streams given back, the code sent. A client whose output
// closed while the command wrote gets 0, as a process would have exited.
int32_t daemon_done(int32_t code) {
  fflush(stdout);
  fflush(stderr);
  for (int i = 0; i < 3; i++)
    if (own_fds[i] >= 0) dup2(own_fds[i], i);
  if (strata_host_closed()) code = 0;
  if (client >= 0 && code != -2) write_all(client, &code, 4);
  drop_client();
  return 0;
}

// (c/cache.c's)
const char *cache_self_path(void);

// This executable's path, its links followed.
const char *daemon_self_path(void) { return cache_self_path(); }
