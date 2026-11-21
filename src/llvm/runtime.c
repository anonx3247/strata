// The native runtime of compiled Strata programs: strings, printing, number
// formatting and faults. Linked with the LLVM IR that `strata build` emits.
// It mirrors the interpreter (src/interp.rs): the same output, the same fault
// messages. Memory is never freed yet (no garbage collector).

#include <dirent.h>
#include <errno.h>
#include <math.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

// A string: its length in bytes, then its UTF-8 bytes (not NUL-terminated).
// String literals are laid out the same way as constants in the IR.
typedef struct {
  int64_t len;
  char bytes[];
} Str;

// Fault kinds, in the order of the IR's constants (`FaultKind` in interp.rs).
static const char *KINDS[] = {"Refinement", "Requires", "Ensures", "Assert", "Runtime", "Hole"};

static Str *str_alloc(int64_t len) {
  Str *s = malloc(sizeof(Str) + (size_t)len);
  if (!s) {
    fputs("fault (Limit): out of memory\n", stderr);
    exit(101);
  }
  s->len = len;
  return s;
}

static Str *str_from(const char *c, int64_t len) {
  Str *s = str_alloc(len);
  memcpy(s->bytes, c, (size_t)len);
  return s;
}

static Str *str_c(const char *c) { return str_from(c, (int64_t)strlen(c)); }

Str *strata_concat(Str *a, Str *b) {
  Str *s = str_alloc(a->len + b->len);
  memcpy(s->bytes, a->bytes, (size_t)a->len);
  memcpy(s->bytes + a->len, b->bytes, (size_t)b->len);
  return s;
}

_Bool strata_str_eq(Str *a, Str *b) { return a->len == b->len && memcmp(a->bytes, b->bytes, (size_t)a->len) == 0; }

int64_t strata_str_cmp(Str *a, Str *b) {
  int64_t n = a->len < b->len ? a->len : b->len;
  int c = memcmp(a->bytes, b->bytes, (size_t)n);
  if (c != 0) return c < 0 ? -1 : 1;
  return a->len < b->len ? -1 : a->len > b->len;
}

// Strings are indexed by character: the length counts code points.
int64_t strata_str_len(Str *s) {
  int64_t n = 0;
  for (int64_t i = 0; i < s->len; i++)
    if ((s->bytes[i] & 0xC0) != 0x80) n++;
  return n;
}

// ---- faults ----

_Noreturn void strata_fault(int32_t kind, Str *msg) {
  fflush(stdout);
  fprintf(stderr, "fault (%s): %.*s\n", KINDS[kind], (int)msg->len, msg->bytes);
  exit(101);
}

_Noreturn void strata_overflow(void) {
  strata_fault(4, str_c("integer overflow (big integers are not compiled yet)"));
}

// ---- numbers ----

int64_t strata_div(int64_t a, int64_t b) {
  if (b == 0) strata_fault(4, str_c("division by zero"));
  if (a == INT64_MIN && b == -1) strata_overflow();
  // Euclidean: the remainder is never negative
  int64_t q = a / b, r = a % b;
  if (r < 0) q = b > 0 ? q - 1 : q + 1;
  return q;
}

int64_t strata_mod(int64_t a, int64_t b) {
  if (b == 0) strata_fault(4, str_c("division by zero"));
  if (b == -1) return 0;
  int64_t r = a % b;
  if (r < 0) r = b > 0 ? r + b : r - b;
  return r;
}

double strata_fmod(double a, double b) {
  double r = fmod(a, b);
  if (r < 0) r += fabs(b);
  return r;
}

Str *strata_show_int(int64_t n) {
  char buf[32];
  return str_c((snprintf(buf, sizeof buf, "%lld", (long long)n), buf));
}

// The shortest decimal digits that read back as x (x is not 0): x is
// 0.DIGITS * 10^kk. Returns whether x is negative.
static int shortest(double x, char digits[32], int *kk) {
  char buf[64];
  for (int prec = 0; prec < 17; prec++) {
    snprintf(buf, sizeof buf, "%.*e", prec, x);
    if (strtod(buf, NULL) == x) break;
  }
  // buf is [-]d[.ddd]e[+-]XX: split into its digits and exponent
  char *p = buf, *out = digits;
  int neg = *p == '-';
  if (neg) p++;
  while (*p != 'e') {
    if (*p != '.') *out++ = *p;
    p++;
  }
  *out = 0;
  *kk = atoi(p + 1) + 1;
  return neg;
}

// A float as serde_json writes it (the shortest digits that read back the
// same, laid out as `ryu` does): 1.0, 0.1, 1e+21, 1.5e-7.
Str *strata_show_float(double x) {
  char digits[32];
  if (x == 0) return str_c(signbit(x) ? "-0.0" : "0.0");
  int kk;
  int neg = shortest(x, digits, &kk);
  int len = (int)strlen(digits);
  int k = kk - len;
  char res[128], *r = res;
  if (neg) *r++ = '-';
  if (k >= 0 && kk <= 16) {
    r += sprintf(r, "%s", digits);
    for (int i = 0; i < k; i++) *r++ = '0';
    r += sprintf(r, ".0");
  } else if (kk > 0 && kk <= 16) {
    r += sprintf(r, "%.*s.%s", kk, digits, digits + kk);
  } else if (kk > -5 && kk <= 0) {
    r += sprintf(r, "0.");
    for (int i = 0; i < -kk; i++) *r++ = '0';
    r += sprintf(r, "%s", digits);
  } else if (len == 1) {
    r += sprintf(r, "%se%+d", digits, kk - 1);
  } else {
    r += sprintf(r, "%c.%se%+d", digits[0], digits + 1, kk - 1);
  }
  return str_from(res, r - res);
}

// A float as Rust's `Display` writes it, in messages: shortest digits,
// never an exponent, no `.0` on whole numbers (1, 0.1, 1000000000000000000000).
static Str *show_float_plain(double x) {
  char digits[32], res[400], *r = res;
  if (x == 0) return str_c(signbit(x) ? "-0" : "0");
  int kk;
  if (shortest(x, digits, &kk)) *r++ = '-';
  int len = (int)strlen(digits);
  if (kk <= 0) {
    r += sprintf(r, "0.");
    for (int i = 0; i < -kk; i++) *r++ = '0';
    r += sprintf(r, "%s", digits);
  } else if (kk >= len) {
    r += sprintf(r, "%s", digits);
    for (int i = len; i < kk; i++) *r++ = '0';
  } else {
    r += sprintf(r, "%.*s.%s", kk, digits, digits + kk);
  }
  return str_from(res, r - res);
}

Str *strata_show_bool(_Bool b) { return str_c(b ? "true" : "false"); }

// A string as JSON (quoted, escaped), for fault messages.
Str *strata_json_str(Str *s) {
  Str *o = str_alloc(s->len * 6 + 2);
  char *p = o->bytes;
  *p++ = '"';
  for (int64_t i = 0; i < s->len; i++) {
    unsigned char c = (unsigned char)s->bytes[i];
    switch (c) {
    case '"': p += sprintf(p, "\\\""); break;
    case '\\': p += sprintf(p, "\\\\"); break;
    case '\n': p += sprintf(p, "\\n"); break;
    case '\r': p += sprintf(p, "\\r"); break;
    case '\t': p += sprintf(p, "\\t"); break;
    case '\b': p += sprintf(p, "\\b"); break;
    case '\f': p += sprintf(p, "\\f"); break;
    default:
      if (c < 0x20) p += sprintf(p, "\\u%04x", c);
      else *p++ = (char)c;
    }
  }
  *p++ = '"';
  o->len = p - o->bytes;
  return o;
}

// A float operation's result must be finite (no NaN or infinity).
double strata_finite(double r, double a, double b, Str *op) {
  if (isfinite(r)) return r;
  Str *m = strata_concat(strata_concat(strata_concat(show_float_plain(a), str_c(" ")), op), str_c(" "));
  strata_fault(4, strata_concat(strata_concat(m, show_float_plain(b)), str_c(" is not a finite number")));
}

// A block for a record, a variant or a tuple (never freed yet).
void *strata_alloc(int64_t n) {
  void *p = malloc((size_t)n);
  if (!p) {
    fputs("fault (Limit): out of memory\n", stderr);
    exit(101);
  }
  return p;
}

// `floor`, `ceil`, `round` of `x` gave `r`, which must fit in an Int.
int64_t strata_float_to_int(double r, double x) {
  if (r >= -9223372036854775808.0 && r < 9223372036854775808.0) return (int64_t)r;
  strata_fault(4, strata_concat(show_float_plain(x), str_c(" is not a whole number")));
}

double strata_sqrt(double x) {
  if (x >= 0) return sqrt(x);
  strata_fault(4, strata_concat(str_c("sqrt of negative number "), show_float_plain(x)));
}

// ---- lists ----
//
// A list is a header (its length, a buffer, and where in the buffer it starts),
// the buffer the elements as 64-bit slots. Lists never change: `push` writes
// in place only past the end of what every list sharing the buffer has used,
// so `out = push(out, x)` in a loop is amortized O(1). The generated code
// reads `len` and the elements directly (the layout is in src/llvm.rs).

typedef struct {
  int64_t cap, used;
  int64_t data[];
} Buf;

typedef struct {
  int64_t len;
  Buf *buf;
  int64_t off;
} List;

static Buf *buf_new(int64_t cap) {
  if (cap < 4) cap = 4;
  Buf *b = strata_alloc((int64_t)sizeof(Buf) + 8 * cap);
  b->cap = cap;
  b->used = 0;
  return b;
}

List *strata_list_new(int64_t cap) {
  List *l = strata_alloc(sizeof(List));
  l->len = 0;
  l->buf = buf_new(cap);
  l->off = 0;
  return l;
}

List *strata_list_push(List *xs, int64_t x) {
  Buf *b = xs->buf;
  int64_t end = xs->off + xs->len;
  List *l = strata_alloc(sizeof(List));
  if (end == b->used && end < b->cap) {
    // nothing else has used the slot after this list's end
    b->data[end] = x;
    b->used++;
    *l = (List){xs->len + 1, b, xs->off};
    return l;
  }
  Buf *n = buf_new(xs->len * 2 + 1);
  memcpy(n->data, b->data + xs->off, (size_t)xs->len * 8);
  n->data[xs->len] = x;
  n->used = xs->len + 1;
  *l = (List){xs->len + 1, n, 0};
  return l;
}

// The elements from `from` to `to` (already within 0..len): shares the buffer.
List *strata_list_slice(List *xs, int64_t from, int64_t to) {
  List *l = strata_alloc(sizeof(List));
  *l = (List){to - from, xs->buf, xs->off + from};
  return l;
}

List *strata_list_concat(List *a, List *b) {
  if (b->len == 0) return a;
  if (a->len == 0) return b;
  Buf *n = buf_new(a->len + b->len);
  memcpy(n->data, a->buf->data + a->off, (size_t)a->len * 8);
  memcpy(n->data + a->len, b->buf->data + b->off, (size_t)b->len * 8);
  n->used = a->len + b->len;
  List *l = strata_alloc(sizeof(List));
  *l = (List){a->len + b->len, n, 0};
  return l;
}

List *strata_range(int64_t lo, int64_t hi) {
  if (hi > lo && (uint64_t)hi - (uint64_t)lo > 1000000) strata_fault(4, str_c("range too large"));
  int64_t n = hi > lo ? hi - lo : 0;
  List *l = strata_list_new(n);
  for (int64_t i = 0; i < n; i++) l->buf->data[i] = lo + i;
  l->buf->used = n;
  l->len = n;
  return l;
}

// `main` returned `Err(e)`.
void strata_main_error(Str *shown) {
  fflush(stdout);
  fprintf(stderr, "error: %.*s\n", (int)shown->len, shown->bytes);
}

// ---- output ----

void strata_print(Str *s) {
  fwrite(s->bytes, 1, (size_t)s->len, stdout);
  fputc('\n', stdout);
}

// ---- the system (the `host_*` operations of std) ----
//
// As the interpreter's (`Interp::host_call`): a failure is a kind and a
// message, the message as Rust writes an OS error ("No such file or
// directory (os error 2)"). The generated code wraps results in `Option`
// and `Result` (whose tags only it knows).

static int prog_argc;
static char **prog_argv;

// The program's arguments, without its name; a closed pipe ends it quietly.
void strata_init(int32_t argc, char **argv) {
  prog_argc = argc > 0 ? argc - 1 : 0;
  prog_argv = argv + 1;
  signal(SIGPIPE, SIG_IGN);
}

static char *cstr(Str *s) {
  char *c = strata_alloc(s->len + 1);
  memcpy(c, s->bytes, (size_t)s->len);
  c[s->len] = 0;
  return c;
}

// The failure in `errno`, as (kind, message).
static void os_error(Str **kind, Str **msg) {
  int e = errno;
  const char *k = "other";
  if (e == ENOENT) k = "not_found";
  else if (e == EACCES || e == EPERM) k = "permission_denied";
  else if (e == EEXIST) k = "already_exists";
  else if (e == ENOTDIR) k = "not_a_directory";
  else if (e == EISDIR) k = "is_a_directory";
  char buf[300];
  snprintf(buf, sizeof buf, "%s (os error %d)", strerror(e), e);
  *kind = str_c(k);
  *msg = str_c(buf);
}

void strata_host_write(int64_t fd, Str *text) {
  FILE *f = fd == 2 ? stderr : stdout;
  if (fd == 2) fflush(stdout);
  if (fwrite(text->bytes, 1, (size_t)text->len, f) < (size_t)text->len || fflush(f) != 0) {
    if (errno == EPIPE) exit(0);
  }
}

// A line without its end, or NULL at the end of the input.
Str *strata_host_read_line(void) {
  size_t cap = 0;
  char *line = NULL;
  ssize_t n = getline(&line, &cap, stdin);
  if (n <= 0) return NULL;
  if (n > 0 && line[n - 1] == '\n') n--;
  if (n > 0 && line[n - 1] == '\r') n--;
  return str_from(line, n);
}

Str *strata_host_read_file(Str *path, Str **kind, Str **msg) {
  FILE *f = fopen(cstr(path), "rb");
  struct stat st;
  if (!f) {
    os_error(kind, msg);
    return NULL;
  }
  if (fstat(fileno(f), &st) == 0 && S_ISDIR(st.st_mode)) {
    errno = EISDIR;
    os_error(kind, msg);
    fclose(f);
    return NULL;
  }
  size_t cap = 4096, len = 0;
  char *buf = malloc(cap);
  size_t got;
  while ((got = fread(buf + len, 1, cap - len, f)) > 0) {
    len += got;
    if (len == cap) buf = realloc(buf, cap *= 2);
  }
  fclose(f);
  Str *s = str_from(buf, (int64_t)len);
  free(buf);
  return s;
}

_Bool strata_host_write_file(Str *path, Str *text, _Bool append, Str **kind, Str **msg) {
  FILE *f = fopen(cstr(path), append ? "ab" : "wb");
  if (!f) {
    os_error(kind, msg);
    return 0;
  }
  if (fwrite(text->bytes, 1, (size_t)text->len, f) < (size_t)text->len) {
    os_error(kind, msg);
    fclose(f);
    return 0;
  }
  fclose(f);
  return 1;
}

_Bool strata_host_exists(Str *path) {
  struct stat st;
  return stat(cstr(path), &st) == 0;
}

static int by_name(const void *a, const void *b) {
  Str *x = *(Str *const *)a, *y = *(Str *const *)b;
  return (int)strata_str_cmp(x, y);
}

// The names in a directory, sorted.
List *strata_host_list_dir(Str *path, Str **kind, Str **msg) {
  DIR *d = opendir(cstr(path));
  if (!d) {
    os_error(kind, msg);
    return NULL;
  }
  List *l = strata_list_new(16);
  struct dirent *e;
  while ((e = readdir(d))) {
    if (!strcmp(e->d_name, ".") || !strcmp(e->d_name, "..")) continue;
    l = strata_list_push(l, (int64_t)(intptr_t)str_c(e->d_name));
  }
  closedir(d);
  qsort(l->buf->data + l->off, (size_t)l->len, 8, by_name);
  return l;
}

// `mkdir -p`.
_Bool strata_host_make_dir(Str *path, Str **kind, Str **msg) {
  char *p = cstr(path);
  for (char *c = p + 1; *c; c++) {
    if (*c != '/') continue;
    *c = 0;
    if (mkdir(p, 0777) != 0 && errno != EEXIST) {
      os_error(kind, msg);
      return 0;
    }
    *c = '/';
  }
  struct stat st;
  if (mkdir(p, 0777) != 0 && !(errno == EEXIST && stat(p, &st) == 0 && S_ISDIR(st.st_mode))) {
    os_error(kind, msg);
    return 0;
  }
  return 1;
}

_Bool strata_host_remove(Str *path, Str **kind, Str **msg) {
  char *p = cstr(path);
  struct stat st;
  int r = stat(p, &st) == 0 && S_ISDIR(st.st_mode) ? rmdir(p) : unlink(p);
  if (r != 0) {
    os_error(kind, msg);
    return 0;
  }
  return 1;
}

List *strata_host_args(void) {
  List *l = strata_list_new(prog_argc);
  for (int i = 0; i < prog_argc; i++) l = strata_list_push(l, (int64_t)(intptr_t)str_c(prog_argv[i]));
  return l;
}

Str *strata_host_env_var(Str *name) {
  const char *v = getenv(cstr(name));
  return v ? str_c(v) : NULL;
}

int64_t strata_pid(void) { return (int64_t)getpid(); }
