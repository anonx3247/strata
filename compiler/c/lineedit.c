// `strata repl`'s line editing at a terminal: a line read with the cursor
// moved by the arrow keys (and Ctrl-A, Ctrl-E, Ctrl-B, Ctrl-F), characters
// deleted (Backspace, Delete, Ctrl-K, Ctrl-U, Ctrl-W), and the lines read
// before brought back by the up and down arrows (Ctrl-P, Ctrl-N). The
// terminal in raw mode only while a line is read. Bytes, not characters:
// a line with multi-byte UTF-8 moves the cursor by bytes (the display may
// lag on such a line; what is read is right).

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <termios.h>
#include <unistd.h>

#define HISTORY 1000

static char *history[HISTORY];
static int history_len;
static char *edited_line;

static void put(const char *s, size_t n) {
  while (n > 0) {
    ssize_t w = write(1, s, n);
    if (w <= 0) return;
    s += w;
    n -= (size_t)w;
  }
}

static void puts_out(const char *s) { put(s, strlen(s)); }

// The line drawn again: the prompt, the text, the cursor at `at`.
static void redraw(const char *prompt, const char *buf, size_t len, size_t at) {
  char move[32];
  puts_out("\r");
  puts_out(prompt);
  put(buf, len);
  puts_out("\x1b[K");
  if (len > at) {
    snprintf(move, sizeof move, "\x1b[%zuD", len - at);
    puts_out(move);
  }
}

// Read one byte: 1, or 0 at the end of the input.
static int byte_in(char *c) { return read(0, c, 1) == 1; }

// A line typed at the terminal after `prompt`, edited; "" at the end of
// the input (Ctrl-D on an empty line: then `repl_edit_ended`). Kept in the
// history when not empty.
const char *repl_edit_line(const char *prompt) {
  struct termios before, raw;
  if (tcgetattr(0, &before) != 0) {
    free(edited_line);
    edited_line = NULL;
    return "";
  }
  raw = before;
  raw.c_lflag &= ~(ECHO | ICANON | IEXTEN | ISIG);
  raw.c_iflag &= ~(IXON | ICRNL);
  raw.c_cc[VMIN] = 1;
  raw.c_cc[VTIME] = 0;
  tcsetattr(0, TCSADRAIN, &raw);
  size_t cap = 256, len = 0, at = 0;
  char *buf = malloc(cap);
  int shown = history_len;  // the history line shown (history_len: the new one)
  char *draft = NULL;       // the new line, kept while the history is shown
  int ended = 0;
  puts_out(prompt);
  for (;;) {
    char c;
    if (!byte_in(&c)) {
      ended = len == 0;
      break;
    }
    if (c == '\r' || c == '\n') break;
    if (c == 4) {  // Ctrl-D: the end on an empty line, else delete
      if (len == 0) {
        ended = 1;
        break;
      }
      if (at < len) {
        memmove(buf + at, buf + at + 1, len - at - 1);
        len--;
      }
    } else if (c == 3) {  // Ctrl-C: the line dropped
      len = at = 0;
      puts_out("^C\r\n");
      puts_out(prompt);
      continue;
    } else if (c == 127 || c == 8) {
      if (at > 0) {
        memmove(buf + at - 1, buf + at, len - at);
        len--;
        at--;
      }
    } else if (c == 1) {
      at = 0;
    } else if (c == 5) {
      at = len;
    } else if (c == 2) {
      if (at > 0) at--;
    } else if (c == 6) {
      if (at < len) at++;
    } else if (c == 11) {
      len = at;
    } else if (c == 21) {
      memmove(buf, buf + at, len - at);
      len -= at;
      at = 0;
    } else if (c == 23) {  // Ctrl-W: the word before the cursor
      size_t from = at;
      while (from > 0 && buf[from - 1] == ' ') from--;
      while (from > 0 && buf[from - 1] != ' ') from--;
      memmove(buf + from, buf + at, len - at);
      len -= at - from;
      at = from;
    } else if (c == 27 || c == 16 || c == 14) {
      char key = c == 16 ? 'A' : c == 14 ? 'B' : 0;
      if (c == 27) {
        char s0, s1;
        if (!byte_in(&s0) || !byte_in(&s1)) break;
        if (s0 == '[' && s1 >= '0' && s1 <= '9') {
          char s2;
          if (!byte_in(&s2)) break;
          if (s1 == '3' && s2 == '~' && at < len) {  // Delete
            memmove(buf + at, buf + at + 1, len - at - 1);
            len--;
          }
        } else if (s0 == '[' || s0 == 'O') {
          key = s1;
        }
      }
      if (key == 'C' && at < len) at++;
      if (key == 'D' && at > 0) at--;
      if (key == 'H') at = 0;
      if (key == 'F') at = len;
      if ((key == 'A' && shown > 0) || (key == 'B' && shown < history_len)) {
        if (shown == history_len) {
          free(draft);
          draft = strndup(buf, len);
        }
        shown += key == 'A' ? -1 : 1;
        const char *s = shown == history_len ? (draft ? draft : "") : history[shown];
        len = at = strlen(s);
        if (len + 1 > cap) buf = realloc(buf, cap = len + 1);
        memcpy(buf, s, len);
      }
    } else if ((unsigned char)c >= 32 || c == '\t') {
      if (len + 2 > cap) buf = realloc(buf, cap *= 2);
      memmove(buf + at + 1, buf + at, len - at);
      buf[at++] = c;
      len++;
    }
    redraw(prompt, buf, len, at);
  }
  tcsetattr(0, TCSADRAIN, &before);
  puts_out("\r\n");
  free(draft);
  free(edited_line);
  edited_line = NULL;
  if (ended) {
    free(buf);
    return "";
  }
  buf[len] = 0;
  edited_line = buf;
  if (len > 0 && (history_len == 0 || strcmp(history[history_len - 1], buf) != 0)) {
    if (history_len == HISTORY) {
      free(history[0]);
      memmove(history, history + 1, (HISTORY - 1) * sizeof *history);
      history_len--;
    }
    history[history_len++] = strdup(buf);
  }
  return edited_line;
}

// Whether the last `repl_edit_line` met the end of the input.
int32_t repl_edit_ended(void) { return edited_line == NULL; }
