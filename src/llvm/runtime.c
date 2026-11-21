// The native runtime of compiled Strata programs: strings, printing, number
// formatting and faults. Linked with the LLVM IR that `strata build` emits.
// It mirrors the interpreter (src/interp.rs): the same output, the same fault
// messages.
//
// Memory is reference-counted (Perceus, as in Koka and Lean): every object
// has a count word just before it, so the layouts below start at the
// object's pointer as if the count weren't there. Constants in the IR have a
// count of 0, which never changes: they are never freed. The generated code
// does the counting (`strata_dup`, `strata_release`, then its own drop of
// the object's contents and `strata_free`); objects never form cycles,
// since values never change once built.

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

// ---- memory ----

#ifdef __APPLE__
#include <malloc/malloc.h>
#define usable_size malloc_size
#else
#include <malloc.h>
#define usable_size malloc_usable_size
#endif


static int64_t allocated, freed;

// With STRATA_MEMORY=check, freed objects are kept, filled with garbage and
// marked, so that touching one again stops the program instead of reading
// whatever reuses the memory.
static _Bool checking;
#define FREED INT64_MIN

// In check mode, every object made (to list those still alive at the end).
static int64_t **made;
static int64_t nmade, capmade;

// The count word of an object.
static int64_t *count_of(void *p) { return (int64_t *)p - 1; }

static _Noreturn void freed_again(const char *what) {
  fflush(stdout);
  fprintf(stderr, "memory: %s of a freed object\n", what);
  abort();
}

// A new object of `n` bytes, held once.
void *strata_alloc(int64_t n) {
  int64_t *c = malloc((size_t)n + 8);
  if (!c) {
    fputs("fault (Limit): out of memory\n", stderr);
    exit(101);
  }
  *c = 1;
  allocated++;
  if (checking) {
    if (nmade == capmade) made = realloc(made, (size_t)(capmade = capmade ? capmade * 2 : 1024) * sizeof *made);
    made[nmade++] = c;
  }
  return c + 1;
}

// One more holder of `p`.
void strata_dup(void *p) {
  int64_t *c = count_of(p);
  if (*c > 0) ++*c;
  else if (*c == FREED) freed_again("dup");
}

// One holder fewer: whether that was the last one, in which case the caller
// drops what the object holds and frees it.
_Bool strata_release(void *p) {
  int64_t *c = count_of(p);
  if (*c == FREED) freed_again("drop");
  return *c > 0 && --*c == 0;
}

// Give an object's memory back (its contents already dropped).
void strata_free(void *p) {
  freed++;
  int64_t *c = count_of(p);
  if (checking) {
    memset(p, 0xDB, usable_size(c) - 8);
    *c = FREED;
    return;
  }
  free(c);
}

// A string has nothing in it to drop.
static void str_drop(void *s) {
  if (strata_release(s)) strata_free(s);
}

// Whether `p` has a single holder, so it may be changed in place.
_Bool strata_unique(void *p) { return *count_of(p) == 1; }

// With STRATA_MEMORY set, how many objects the program made and freed.
// In check mode, the first few still alive: their count, size and first
// word (a block's tag, a string's or a list's length), as text if it reads
// as a string.
static void memory_stats(void) {
  fprintf(stderr, "memory: %lld allocated, %lld freed, %lld live\n", (long long)allocated, (long long)freed, (long long)(allocated - freed));
  int shown = 0;
  for (int64_t i = 0; i < nmade && shown < 20; i++) {
    int64_t *c = made[i];
    if (*c == FREED) continue;
    shown++;
    size_t size = usable_size(c) - 8;
    int64_t first = c[1];
    fprintf(stderr, "  live: count %lld, %zu bytes, first word %lld", (long long)*c, size, (long long)first);
    if (first >= 0 && (size_t)first + 8 <= size) fprintf(stderr, " \"%.*s\"", (int)(first < 40 ? first : 40), (char *)(c + 2));
    fputc('\n', stderr);
  }
}

static Str *str_alloc(int64_t len) {
  Str *s = strata_alloc((int64_t)sizeof(Str) + len);
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

// `a` then `b`, which the caller hands over (both are dropped).
Str *strata_concat_owned(Str *a, Str *b) {
  Str *s = strata_concat(a, b);
  str_drop(a);
  str_drop(b);
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
//
// Headers and buffers are counted objects: a header holds its buffer, and a
// buffer holds its elements up to `used`. These functions borrow the lists
// they are given; when the elements are counted objects (`counted`), each
// one copied into a new buffer is held once more. The generated code drops
// lists (it knows the elements' types).

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

static void dup_slots(int64_t *xs, int64_t n, _Bool counted) {
  if (counted)
    for (int64_t i = 0; i < n; i++) strata_dup((void *)(intptr_t)xs[i]);
}

// `xs` with `x` after it (`x` is handed over).
List *strata_list_push(List *xs, int64_t x, _Bool counted) {
  Buf *b = xs->buf;
  int64_t end = xs->off + xs->len;
  List *l = strata_alloc(sizeof(List));
  if (end == b->used && end < b->cap) {
    // nothing else has used the slot after this list's end
    b->data[end] = x;
    b->used++;
    strata_dup(b);
    *l = (List){xs->len + 1, b, xs->off};
    return l;
  }
  Buf *n = buf_new(xs->len * 2 + 1);
  memcpy(n->data, b->data + xs->off, (size_t)xs->len * 8);
  dup_slots(n->data, xs->len, counted);
  n->data[xs->len] = x;
  n->used = xs->len + 1;
  *l = (List){xs->len + 1, n, 0};
  return l;
}

// Add `x` to a list nothing else holds yet (one the runtime is making).
static List *append(List *l, int64_t x) {
  Buf *b = l->buf;
  if (b->used == b->cap) {
    Buf *n = buf_new(b->cap * 2);
    memcpy(n->data, b->data, (size_t)b->used * 8);
    n->used = b->used;
    strata_free(b);
    l->buf = b = n;
  }
  b->data[b->used++] = x;
  l->len++;
  return l;
}

// The elements from `from` to `to` (already within 0..len): shares the buffer.
List *strata_list_slice(List *xs, int64_t from, int64_t to) {
  List *l = strata_alloc(sizeof(List));
  strata_dup(xs->buf);
  *l = (List){to - from, xs->buf, xs->off + from};
  return l;
}

List *strata_list_concat(List *a, List *b, _Bool counted) {
  if (b->len == 0) return strata_dup(a), a;
  if (a->len == 0) return strata_dup(b), b;
  Buf *n = buf_new(a->len + b->len);
  memcpy(n->data, a->buf->data + a->off, (size_t)a->len * 8);
  memcpy(n->data + a->len, b->buf->data + b->off, (size_t)b->len * 8);
  dup_slots(n->data, a->len + b->len, counted);
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
// The program's name, as `--help` shows it: its file's, without the folder.
static char *prog_name = "main";
static char **prog_argv;

// The program's arguments, without its name; a closed pipe ends it quietly.
void strata_init(int32_t argc, char **argv) {
  prog_argc = argc > 0 ? argc - 1 : 0;
  prog_argv = argv + 1;
  if (argc > 0 && argv[0]) prog_name = argv[0];
  signal(SIGPIPE, SIG_IGN);
  allocated = freed = 0;
  static _Bool registered;
  const char *m = getenv("STRATA_MEMORY");
  checking = m && !strcmp(m, "check");
  if (m && !registered) registered = 1, atexit(memory_stats);
}

Str *strata_prog_name(void) {
  const char *slash = strrchr(prog_name, '/');
  return str_c(slash ? slash + 1 : prog_name);
}

// The options could not be read: the help asked for (on standard output), or
// what is wrong with the arguments (on standard error).
void strata_options_error(_Bool help, Str *text) {
  if (help) {
    fwrite(text->bytes, 1, (size_t)text->len, stdout);
    fflush(stdout);
  } else {
    fflush(stdout);
    fprintf(stderr, "error: %.*s\n", (int)text->len, text->bytes);
  }
}

// A C copy of `s`, for the OS: valid until the next call (one buffer, reused).
static char *cstr(Str *s) {
  static char *c;
  static size_t cap;
  if ((size_t)s->len + 1 > cap) c = realloc(c, cap = (size_t)s->len + 1);
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
    l = append(l, (int64_t)(intptr_t)str_c(e->d_name));
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
  for (int i = 0; i < prog_argc; i++) l = append(l, (int64_t)(intptr_t)str_c(prog_argv[i]));
  return l;
}

Str *strata_host_env_var(Str *name) {
  const char *v = getenv(cstr(name));
  return v ? str_c(v) : NULL;
}

int64_t strata_pid(void) { return (int64_t)getpid(); }

// ---- strings ----
//
// The string built-ins (`interp::Interp::builtin`). Strings are UTF-8 and
// indexed by character (code point). What depends on Unicode's tables (case
// mappings, `is_alpha`) takes a table the compiler made from Rust's own
// (`llvm.rs`, `unicode_table`), so both agree.

// The character at byte `i` (valid UTF-8), and its length in bytes.
static int32_t utf8_at(Str *s, int64_t i, int *n) {
  unsigned char c = (unsigned char)s->bytes[i];
  if (c < 0x80) return *n = 1, c;
  if (c < 0xE0) return *n = 2, ((c & 0x1F) << 6) | (s->bytes[i + 1] & 0x3F);
  if (c < 0xF0) return *n = 3, ((c & 0x0F) << 12) | ((s->bytes[i + 1] & 0x3F) << 6) | (s->bytes[i + 2] & 0x3F);
  return *n = 4, ((c & 0x07) << 18) | ((s->bytes[i + 1] & 0x3F) << 12) | ((s->bytes[i + 2] & 0x3F) << 6) | (s->bytes[i + 3] & 0x3F);
}

static int utf8_put(int32_t c, char *o) {
  if (c < 0x80) return o[0] = (char)c, 1;
  if (c < 0x800) return o[0] = (char)(0xC0 | (c >> 6)), o[1] = (char)(0x80 | (c & 0x3F)), 2;
  if (c < 0x10000) return o[0] = (char)(0xE0 | (c >> 12)), o[1] = (char)(0x80 | ((c >> 6) & 0x3F)), o[2] = (char)(0x80 | (c & 0x3F)), 3;
  o[0] = (char)(0xF0 | (c >> 18)), o[1] = (char)(0x80 | ((c >> 12) & 0x3F)), o[2] = (char)(0x80 | ((c >> 6) & 0x3F)), o[3] = (char)(0x80 | (c & 0x3F));
  return 4;
}

// The byte where character `k` starts (the end, past the last one).
static int64_t char_offset(Str *s, int64_t k) {
  int64_t i = 0;
  while (k > 0 && i < s->len) {
    int n;
    utf8_at(s, i, &n);
    i += n;
    k--;
  }
  return i;
}

// The first place `sub` occurs in `s` at or after byte `from`, or -1.
static int64_t find_from(Str *s, Str *sub, int64_t from) {
  for (int64_t i = from; i + sub->len <= s->len; i++)
    if (memcmp(s->bytes + i, sub->bytes, (size_t)sub->len) == 0) return i;
  return -1;
}

static List *push_str(List *l, Str *s) { return append(l, (int64_t)(intptr_t)s); }

List *strata_str_split(Str *s, Str *sep) {
  if (sep->len == 0) strata_fault(4, str_c("split: empty separator"));
  List *l = strata_list_new(4);
  int64_t start = 0, at;
  while ((at = find_from(s, sep, start)) >= 0) {
    l = push_str(l, str_from(s->bytes + start, at - start));
    start = at + sep->len;
  }
  return push_str(l, str_from(s->bytes + start, s->len - start));
}

// Unicode's White_Space, as Rust's `char::is_whitespace`.
static int is_space(int32_t c) {
  return (c >= 9 && c <= 13) || c == 32 || c == 0x85 || c == 0xA0 || c == 0x1680 || (c >= 0x2000 && c <= 0x200A) || c == 0x2028 || c == 0x2029 || c == 0x202F || c == 0x205F || c == 0x3000;
}

Str *strata_str_trim(Str *s) {
  int64_t a = 0, b = s->len;
  while (a < b) {
    int n;
    if (!is_space(utf8_at(s, a, &n))) break;
    a += n;
  }
  while (b > a) {
    int64_t p = b - 1;
    while ((s->bytes[p] & 0xC0) == 0x80) p--;
    int n;
    if (!is_space(utf8_at(s, p, &n))) break;
    b = p;
  }
  return str_from(s->bytes + a, b - a);
}

_Bool strata_str_contains(Str *s, Str *sub) { return find_from(s, sub, 0) >= 0; }

_Bool strata_str_starts_with(Str *s, Str *p) { return p->len <= s->len && memcmp(s->bytes, p->bytes, (size_t)p->len) == 0; }

_Bool strata_str_ends_with(Str *s, Str *p) { return p->len <= s->len && memcmp(s->bytes + s->len - p->len, p->bytes, (size_t)p->len) == 0; }

// The character index of the first `sub`, or -1.
int64_t strata_str_index_of(Str *s, Str *sub) {
  int64_t at = find_from(s, sub, 0);
  if (at < 0) return -1;
  int64_t k = 0;
  for (int64_t i = 0; i < at; i++)
    if ((s->bytes[i] & 0xC0) != 0x80) k++;
  return k;
}

Str *strata_str_reverse(Str *s) {
  Str *o = str_alloc(s->len);
  int64_t i = 0;
  while (i < s->len) {
    int n;
    utf8_at(s, i, &n);
    memcpy(o->bytes + s->len - i - n, s->bytes + i, (size_t)n);
    i += n;
  }
  return o;
}

List *strata_str_chars(Str *s) {
  List *l = strata_list_new(s->len);
  int64_t i = 0;
  while (i < s->len) {
    int n;
    utf8_at(s, i, &n);
    l = push_str(l, str_from(s->bytes + i, n));
    i += n;
  }
  return l;
}

// Character `k`, or NULL when there is none.
Str *strata_str_at(Str *s, int64_t k) {
  if (k < 0) return NULL;
  int64_t i = char_offset(s, k);
  if (i >= s->len) return NULL;
  int n;
  utf8_at(s, i, &n);
  return str_from(s->bytes + i, n);
}

// `xs[i]` on a string: character `k`, or a fault.
Str *strata_str_at_or_fault(Str *s, int64_t k) {
  Str *c = strata_str_at(s, k);
  if (c) return c;
  char buf[96];
  snprintf(buf, sizeof buf, "index %lld out of range (length %lld)", (long long)k, (long long)strata_str_len(s));
  strata_fault(4, str_c(buf));
}

// Characters `a` to `b`, clamped to the string (as in Python).
Str *strata_str_slice(Str *s, int64_t a, int64_t b) {
  if (a < 0) a = 0;
  if (b <= a) return str_from("", 0);
  int64_t i = char_offset(s, a), j = i;
  for (int64_t k = a; k < b && j < s->len; k++) {
    int n;
    utf8_at(s, j, &n);
    j += n;
  }
  return str_from(s->bytes + i, j - i);
}

Str *strata_str_replace(Str *s, Str *from, Str *to) {
  if (from->len == 0) strata_fault(4, str_c("replace: empty pattern"));
  Str *out = str_from("", 0);
  int64_t start = 0, at;
  while ((at = find_from(s, from, start)) >= 0) {
    out = strata_concat_owned(out, str_from(s->bytes + start, at - start));
    Str *next = strata_concat(out, to);
    str_drop(out);
    out = next;
    start = at + from->len;
  }
  return strata_concat_owned(out, str_from(s->bytes + start, s->len - start));
}

Str *strata_str_repeat(Str *s, int64_t n) {
  if (n < 0 || (s->len > 0 && n > 100000000 / s->len)) {
    char buf[64];
    snprintf(buf, sizeof buf, "repeat: bad count %lld", (long long)n);
    strata_fault(4, str_c(buf));
  }
  Str *o = str_alloc(s->len * n);
  for (int64_t i = 0; i < n; i++) memcpy(o->bytes + i * s->len, s->bytes, (size_t)s->len);
  return o;
}

// Whether `c` is in a table of ranges (pairs of first and last).
static int in_ranges(int32_t c, const int32_t *r, int64_t n) {
  int64_t lo = 0, hi = n;
  while (lo < hi) {
    int64_t mid = (lo + hi) / 2;
    if (c < r[2 * mid]) hi = mid;
    else if (c > r[2 * mid + 1]) lo = mid + 1;
    else return 1;
  }
  return 0;
}

// The mapping of `c` in a case table (rows of a character and the up to
// three it becomes), or NULL when it maps to itself.
static const int32_t *case_row(int32_t c, const int32_t *t, int64_t n) {
  int64_t lo = 0, hi = n;
  while (lo < hi) {
    int64_t mid = (lo + hi) / 2;
    if (c < t[4 * mid]) hi = mid;
    else if (c > t[4 * mid]) lo = mid + 1;
    else return t + 4 * mid + 1;
  }
  return NULL;
}

// `to_upper` / `to_lower`, with Rust's mappings (`table`). Lowercase `Σ` is
// `ς` at the end of a word: after a cased character (`cased`), and not
// before one.
Str *strata_str_case(Str *s, const int32_t *table, int64_t n, const int32_t *cased, int64_t m, _Bool lower) {
  Str *o = str_alloc(s->len * 3);
  char *p = o->bytes;
  int32_t prev = -1;
  int64_t i = 0;
  while (i < s->len) {
    int k;
    int32_t c = utf8_at(s, i, &k);
    if (lower && c == 0x3A3) {
      int final = prev >= 0 && in_ranges(prev, cased, m);
      if (final && i + k < s->len) {
        int k2;
        final = !in_ranges(utf8_at(s, i + k, &k2), cased, m);
      }
      p += utf8_put(final ? 0x3C2 : 0x3C3, p);
    } else {
      const int32_t *row = case_row(c, table, n);
      if (!row) p += utf8_put(c, p);
      else
        for (int j = 0; j < 3 && row[j]; j++) p += utf8_put(row[j], p);
    }
    prev = c;
    i += k;
  }
  o->len = p - o->bytes;
  return o;
}

// `is_digit` (0), `is_alpha` (1, `ranges` from Unicode's Alphabetic),
// `is_space` (2): not empty, and every character of the class.
_Bool strata_str_is(Str *s, int64_t class, const int32_t *ranges, int64_t n) {
  if (s->len == 0) return 0;
  int64_t i = 0;
  while (i < s->len) {
    int k;
    int32_t c = utf8_at(s, i, &k);
    int ok = class == 0 ? (c >= '0' && c <= '9') : class == 1 ? in_ranges(c, ranges, n) : is_space(c);
    if (!ok) return 0;
    i += k;
  }
  return 1;
}

// A string as Rust's `{:?}` writes it, in messages.
static Str *debug_str(Str *s) {
  Str *o = str_alloc(s->len * 10 + 2);
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
    case 0: p += sprintf(p, "\\0"); break;
    default:
      if (c < 0x20 || c == 0x7F) p += sprintf(p, "\\u{%x}", c);
      else *p++ = (char)c;
    }
  }
  *p++ = '"';
  o->len = p - o->bytes;
  return o;
}

int64_t strata_str_ord(Str *s) {
  int n;
  if (s->len > 0) {
    int32_t c = utf8_at(s, 0, &n);
    if (n == s->len) return c;
  }
  strata_fault(4, strata_concat(str_c("ord needs a single character, got "), debug_str(s)));
}

Str *strata_str_chr(int64_t c) {
  if (c < 0 || c > 0x10FFFF || (c >= 0xD800 && c <= 0xDFFF)) {
    char buf[64];
    snprintf(buf, sizeof buf, "chr: %lld is not a character", (long long)c);
    strata_fault(4, str_c(buf));
  }
  char b[4];
  return str_from(b, utf8_put((int32_t)c, b));
}

// `parse_int`, as Rust's `BigInt` reads the trimmed string: a sign, then a
// digit, then digits and `_`. False when it isn't a number; a fault when it
// doesn't fit in 64 bits (big integers are not compiled yet).
static _Bool parse_int_trimmed(Str *t, int64_t *out);
static _Bool parse_float_trimmed(Str *t, double *out);

_Bool strata_parse_int(Str *s, int64_t *out) {
  Str *t = strata_str_trim(s);
  _Bool ok = parse_int_trimmed(t, out);
  str_drop(t);
  return ok;
}

_Bool strata_parse_float(Str *s, double *out) {
  Str *t = strata_str_trim(s);
  _Bool ok = parse_float_trimmed(t, out);
  str_drop(t);
  return ok;
}

static _Bool parse_int_trimmed(Str *t, int64_t *out) {
  int64_t i = 0, n = t->len;
  int neg = 0;
  if (i < n && (t->bytes[i] == '+' || t->bytes[i] == '-')) neg = t->bytes[i++] == '-';
  if (i >= n || t->bytes[i] < '0' || t->bytes[i] > '9') return 0;
  uint64_t v = 0;
  int big = 0;
  for (; i < n; i++) {
    char c = t->bytes[i];
    if (c == '_') continue;
    if (c < '0' || c > '9') return 0;
    if (v > (UINT64_MAX - (uint64_t)(c - '0')) / 10) big = 1;
    else v = v * 10 + (uint64_t)(c - '0');
  }
  if (big || v > (uint64_t)INT64_MAX + (uint64_t)neg) strata_overflow();
  *out = neg ? (int64_t)(0 - v) : (int64_t)v;
  return 1;
}

// `parse_float`, as Rust's `f64::from_str` reads the trimmed string (no hex,
// no `_`; `inf` and `NaN` are not finite, so not numbers either).
static _Bool parse_float_trimmed(Str *t, double *out) {
  int64_t i = 0, n = t->len, digits = 0;
  const char *b = t->bytes;
  if (i < n && (b[i] == '+' || b[i] == '-')) i++;
  while (i < n && b[i] >= '0' && b[i] <= '9') i++, digits++;
  if (i < n && b[i] == '.') {
    i++;
    while (i < n && b[i] >= '0' && b[i] <= '9') i++, digits++;
  }
  if (digits == 0) return 0;
  if (i < n && (b[i] == 'e' || b[i] == 'E')) {
    i++;
    if (i < n && (b[i] == '+' || b[i] == '-')) i++;
    int64_t e = 0;
    while (i < n && b[i] >= '0' && b[i] <= '9') i++, e++;
    if (e == 0) return 0;
  }
  if (i != n) return 0;
  double x = strtod(cstr(t), NULL);
  if (!isfinite(x)) return 0;
  *out = x;
  return 1;
}

// ---- f-string format specs (`interp::format_spec`) ----

// A float with `p` decimals, as Rust's `{:.p}`.
Str *strata_show_float_fixed(double x, int64_t p) {
  int n = snprintf(NULL, 0, "%.*f", (int)p, x);
  Str *s = str_alloc(n);
  char *tmp = malloc((size_t)n + 1);
  snprintf(tmp, (size_t)n + 1, "%.*f", (int)p, x);
  memcpy(s->bytes, tmp, (size_t)n);
  free(tmp);
  return s;
}

// `body` padded to `width` characters: `fill` and `align` (`<`, `>`, `^`, or
// 0 for the default: numbers right, the rest left), `+` for a number's sign,
// `0` to pad a number with zeros after its sign.
// `body` is handed over.
Str *strata_format(Str *body, _Bool numeric, int32_t fill, int32_t align, _Bool plus, _Bool zero, int64_t width) {
  if (plus && !(body->len > 0 && body->bytes[0] == '-')) body = strata_concat_owned(str_c("+"), body);
  int64_t n = strata_str_len(body);
  if (n >= width) return body;
  int64_t pad = width - n;
  if (zero && align == 0) {
    int sign = body->len > 0 && (body->bytes[0] == '+' || body->bytes[0] == '-');
    Str *o = str_alloc(body->len + pad);
    memcpy(o->bytes, body->bytes, (size_t)sign);
    memset(o->bytes + sign, '0', (size_t)pad);
    memcpy(o->bytes + sign + pad, body->bytes + sign, (size_t)(body->len - sign));
    str_drop(body);
    return o;
  }
  char f[4];
  int fl = utf8_put(fill, f);
  if (align == 0) align = numeric ? '>' : '<';
  int64_t left = align == '<' ? 0 : align == '>' ? pad : pad / 2, right = pad - left;
  Str *o = str_alloc(body->len + pad * fl);
  char *p = o->bytes;
  for (int64_t i = 0; i < left; i++) memcpy(p, f, (size_t)fl), p += fl;
  memcpy(p, body->bytes, (size_t)body->len), p += body->len;
  for (int64_t i = 0; i < right; i++) memcpy(p, f, (size_t)fl), p += fl;
  str_drop(body);
  return o;
}
