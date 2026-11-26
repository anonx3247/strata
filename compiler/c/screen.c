// `strata tour`'s screen (compiler/tour): the terminal in raw mode on the
// alternate screen, its size, a frame drawn at once, and keys read one at a
// time. The terminal is put back as it was by `screen_end`, at exit, and on
// a signal that ends the process (Ctrl-C reads as a key while the screen is
// on).

#include <errno.h>
#include <poll.h>
#include <signal.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <termios.h>
#include <unistd.h>

static struct termios saved;
static int active;
static volatile sig_atomic_t resized;

static void put(const char *s, size_t n) {
  while (n > 0) {
    ssize_t w = write(1, s, n);
    if (w < 0 && errno == EINTR) continue;
    if (w <= 0) return;
    s += w;
    n -= (size_t)w;
  }
}

static void puts_out(const char *s) { put(s, strlen(s)); }

static void restore(void) {
  if (!active) return;
  active = 0;
  tcsetattr(0, TCSAFLUSH, &saved);
  puts_out("\x1b[?25h\x1b[?1049l");
}

static void on_signal(int sig) {
  restore();
  signal(sig, SIG_DFL);
  raise(sig);
}

static void on_resize(int sig) {
  (void)sig;
  resized = 1;
}

// 1 when standard input and output are both a terminal, else 0.
int32_t screen_interactive(void) { return isatty(0) && isatty(1) ? 1 : 0; }

static int64_t size_of(int rows) {
  struct winsize w;
  if (!isatty(1) || ioctl(1, TIOCGWINSZ, &w) != 0) return 0;
  return rows ? (int64_t)w.ws_row : (int64_t)w.ws_col;
}

// The terminal's rows and columns, 0 when standard output is not one.
int64_t screen_rows(void) { return size_of(1); }
int64_t screen_columns(void) { return size_of(0); }

// Raw mode (no echo, keys one at a time, Ctrl-C a key), the alternate
// screen, the cursor hidden. 0 when done, 1 when not at a terminal.
int32_t screen_begin(void) {
  if (active) return 0;
  if (!screen_interactive() || tcgetattr(0, &saved) != 0) return 1;
  struct termios raw = saved;
  raw.c_lflag &= ~(ECHO | ICANON | IEXTEN | ISIG);
  raw.c_iflag &= ~(IXON | ICRNL);
  raw.c_cc[VMIN] = 1;
  raw.c_cc[VTIME] = 0;
  if (tcsetattr(0, TCSAFLUSH, &raw) != 0) return 1;
  active = 1;
  static int hooked;
  if (!hooked) {
    hooked = 1;
    atexit(restore);
    signal(SIGTERM, on_signal);
    signal(SIGHUP, on_signal);
    signal(SIGINT, on_signal);
    signal(SIGQUIT, on_signal);
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = on_resize;  // no SA_RESTART: a read in `screen_key` returns
    sigaction(SIGWINCH, &sa, NULL);
  }
  puts_out("\x1b[?1049h\x1b[?25l");
  return 0;
}

// The terminal back as it was. 0.
int32_t screen_end(void) {
  restore();
  return 0;
}

// `text` drawn from the top left, each line cleared to its end, then the
// rest of the screen cleared. 0.
int32_t screen_draw(const char *text) {
  size_t n = strlen(text);
  char *out = malloc(n * 5 + 16);
  if (out == NULL) return 0;
  size_t k = 0;
  memcpy(out + k, "\x1b[H", 3);
  k += 3;
  for (size_t i = 0; i < n; i++) {
    if (text[i] == '\n') {
      memcpy(out + k, "\x1b[K\r\n", 5);
      k += 5;
    } else {
      out[k++] = text[i];
    }
  }
  memcpy(out + k, "\x1b[K\x1b[J", 6);
  k += 6;
  put(out, k);
  free(out);
  return 0;
}

// One byte, waiting at most `ms` milliseconds (-1: as long as it takes).
// -1 when none came in time, -2 when the read ended (end of the input, or a
// signal: `errno` says).
static int byte_in(int ms) {
  if (ms >= 0) {
    struct pollfd p = {.fd = 0, .events = POLLIN};
    if (poll(&p, 1, ms) <= 0) return -1;
  }
  unsigned char c;
  errno = 0;
  ssize_t r = read(0, &c, 1);
  if (r == 1) return c;
  return -2;
}

// The next key: `up`, `down`, `left`, `right`, `pgup`, `pgdown`, `home`,
// `end`, `enter`, `tab`, `space`, `escape`, `backspace`, `ctrl-c`, a
// character as itself; `resize` when the terminal changed size, `""` at
// the end of the input (or Ctrl-D).
const char *screen_key(void) {
  static char one[2];
  for (;;) {
    if (resized) {
      resized = 0;
      return "resize";
    }
    int c = byte_in(-1);
    if (c == -2) {
      if (errno == EINTR) continue;
      return "";
    }
    if (c == 3) return "ctrl-c";
    if (c == 4) return "";
    if (c == '\r' || c == '\n') return "enter";
    if (c == '\t') return "tab";
    if (c == ' ') return "space";
    if (c == 127 || c == 8) return "backspace";
    if (c != 27) {
      if (c < 32 || c > 126) continue;
      one[0] = (char)c;
      one[1] = 0;
      return one;
    }
    int a = byte_in(30);
    if (a != '[' && a != 'O') return "escape";
    int b = byte_in(30);
    switch (b) {
      case 'A': return "up";
      case 'B': return "down";
      case 'C': return "right";
      case 'D': return "left";
      case 'H': return "home";
      case 'F': return "end";
    }
    if (b >= '0' && b <= '9') {
      int t = byte_in(30);
      while (t >= 0 && t != '~' && !(t >= 'A' && t <= 'Z') && !(t >= 'a' && t <= 'z')) t = byte_in(30);
      if (t == '~') {
        switch (b) {
          case '1': case '7': return "home";
          case '4': case '8': return "end";
          case '5': return "pgup";
          case '6': return "pgdown";
        }
      }
    }
    // the sequence of some other key: read on
  }
}
