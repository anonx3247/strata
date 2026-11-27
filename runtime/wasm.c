// The C library of a WebAssembly module (`strata build --target wasm64`):
// what runtime.c, core.rt and compiled code call of one, for a module that
// runs on one thread and reaches the outside only through its host. Linked
// with runtime.c built with STRATA_WASM. The host gives the imports below
// (`env`): output, float text, ending; the loader `derive TypeScript` writes
// gives them all.
#include "wasm.h"

// ---- the host ----

__attribute__((import_module("env"), import_name("strata_wasm_write")))
void host_write(int32_t fd, const void *p, int64_t n);
// x as C's `%.*e` (conv 'e') or `%.*f` (conv 'f'), into out; its length
__attribute__((import_module("env"), import_name("strata_wasm_float")))
int32_t host_float(double x, int32_t conv, int32_t prec, char *out, int64_t cap);
// the float at the start of s (n bytes), as strtod reads it; how many bytes
// it took in *used (0 for none)
__attribute__((import_module("env"), import_name("strata_wasm_strtod")))
double host_strtod(const char *s, int64_t n, int64_t *used);
// a fault (kind as in KINDS) or an exit: the host throws, so these never return
__attribute__((import_module("env"), import_name("strata_wasm_fault")))
void host_fault(int32_t kind, const char *msg, int64_t n);
__attribute__((import_module("env"), import_name("strata_wasm_exit")))
void host_exit(int32_t code);

// ---- memory: size classes over memory.grow ----
//
// Each block has a 16-byte header holding its class; classes are powers of
// two from 16 bytes. A freed block goes on its class's list. Memory grows a
// 64 KB page at a time and is never given back.

extern unsigned char __heap_base;
static unsigned char *top, *end;
static void *lists[48];

typedef struct {
  uint64_t cls, pad;
} Head;

static int class_of(size_t n) {
  int k = 4;
  while (((size_t)1 << k) < n) k++;
  return k;
}

static void *carve(size_t bytes) {
  if (!top) top = end = (unsigned char *)(((uintptr_t)&__heap_base + 15) & ~(uintptr_t)15);
  if ((size_t)(end - top) < bytes) {
    size_t pages = (bytes - (size_t)(end - top) + 65535) / 65536;
    if (__builtin_wasm_memory_grow(0, pages) == (size_t)-1) return NULL;
    end += pages * 65536;
  }
  void *p = top;
  top += bytes;
  return p;
}

void *malloc(size_t n) {
  int k = class_of(n ? n : 1);
  Head *h = lists[k];
  if (h) {
    lists[k] = *(void **)(h + 1);
  } else {
    h = carve(sizeof(Head) + ((size_t)1 << k));
    if (!h) return NULL;
  }
  h->cls = (uint64_t)k;
  return h + 1;
}

void free(void *p) {
  if (!p) return;
  Head *h = (Head *)p - 1;
  *(void **)p = lists[h->cls];
  lists[h->cls] = h;
}

size_t malloc_usable_size(void *p) { return p ? (size_t)1 << ((Head *)p - 1)->cls : 0; }

void *calloc(size_t n, size_t k) {
  void *p = malloc(n * k);
  if (p) memset(p, 0, n * k);
  return p;
}

void *realloc(void *p, size_t n) {
  if (!p) return malloc(n);
  size_t had = malloc_usable_size(p);
  if (n <= had) return p;
  void *q = malloc(n);
  if (q) {
    memcpy(q, p, had);
    free(p);
  }
  return q;
}

int posix_memalign(void **out, size_t align, size_t n) {
  // (blocks are 16-aligned; a larger alignment gets a block that holds one)
  if (align <= 16) {
    *out = malloc(n);
    return *out ? 0 : ENOMEM;
  }
  return ENOMEM;
}

// The memory a host writes arguments into (`derive TypeScript`'s loader).
__attribute__((export_name("strata_wasm_alloc"))) void *strata_wasm_alloc(int64_t n) { return malloc((size_t)n); }
__attribute__((export_name("strata_wasm_free"))) void strata_wasm_free(void *p) { free(p); }

// ---- bytes and strings ----

void *memcpy(void *d, const void *s, size_t n) {
  unsigned char *a = d;
  const unsigned char *b = s;
  while (n--) *a++ = *b++;
  return d;
}

void *memmove(void *d, const void *s, size_t n) {
  unsigned char *a = d;
  const unsigned char *b = s;
  if (a < b) {
    while (n--) *a++ = *b++;
  } else {
    while (n--) a[n] = b[n];
  }
  return d;
}

void *memset(void *d, int c, size_t n) {
  unsigned char *a = d;
  while (n--) *a++ = (unsigned char)c;
  return d;
}

int memcmp(const void *a, const void *b, size_t n) {
  const unsigned char *x = a, *y = b;
  for (; n--; x++, y++)
    if (*x != *y) return *x - *y;
  return 0;
}

void *memchr(const void *s, int c, size_t n) {
  const unsigned char *p = s;
  for (; n--; p++)
    if (*p == (unsigned char)c) return (void *)p;
  return NULL;
}

size_t strlen(const char *s) {
  size_t n = 0;
  while (s[n]) n++;
  return n;
}

int strcmp(const char *a, const char *b) {
  while (*a && *a == *b) a++, b++;
  return (unsigned char)*a - (unsigned char)*b;
}

int strncmp(const char *a, const char *b, size_t n) {
  for (; n && *a && *a == *b; n--) a++, b++;
  return n ? (unsigned char)*a - (unsigned char)*b : 0;
}

char *strchr(const char *s, int c) {
  for (;; s++) {
    if (*s == (char)c) return (char *)s;
    if (!*s) return NULL;
  }
}

char *strrchr(const char *s, int c) {
  const char *r = NULL;
  for (;; s++) {
    if (*s == (char)c) r = s;
    if (!*s) return (char *)r;
  }
}

char *strstr(const char *h, const char *n) {
  size_t k = strlen(n);
  for (; *h; h++)
    if (strncmp(h, n, k) == 0) return (char *)h;
  return k ? NULL : (char *)h;
}

char *strndup(const char *s, size_t n) {
  size_t k = 0;
  while (k < n && s[k]) k++;
  char *r = malloc(k + 1);
  memcpy(r, s, k);
  r[k] = 0;
  return r;
}

char *strdup(const char *s) { return strndup(s, strlen(s)); }

long long strtoll(const char *s, char **endp, int base) {
  while (*s == ' ' || *s == '\t' || *s == '\n') s++;
  int neg = *s == '-';
  if (*s == '-' || *s == '+') s++;
  if (base == 0) base = 10;
  unsigned long long v = 0;
  for (;; s++) {
    int d = *s >= '0' && *s <= '9' ? *s - '0' : *s >= 'a' && *s <= 'z' ? *s - 'a' + 10 : *s >= 'A' && *s <= 'Z' ? *s - 'A' + 10 : 99;
    if (d >= base) break;
    v = v * (unsigned)base + (unsigned)d;
  }
  if (endp) *endp = (char *)s;
  return neg ? -(long long)v : (long long)v;
}

long strtol(const char *s, char **endp, int base) { return (long)strtoll(s, endp, base); }
unsigned long long strtoull(const char *s, char **endp, int base) { return (unsigned long long)strtoll(s, endp, base); }
int atoi(const char *s) { return (int)strtoll(s, NULL, 10); }

double strtod(const char *s, char **endp) {
  int64_t used = 0;
  double x = host_strtod(s, (int64_t)strlen(s), &used);
  if (endp) *endp = (char *)s + used;
  return x;
}

void qsort(void *base, size_t n, size_t size, int (*cmp)(const void *, const void *)) {
  // (insertion sort: the runtime sorts short lists of reports only)
  unsigned char *b = base, *t = malloc(size);
  for (size_t i = 1; i < n; i++) {
    memcpy(t, b + i * size, size);
    size_t j = i;
    while (j > 0 && cmp(b + (j - 1) * size, t) > 0) {
      memcpy(b + j * size, b + (j - 1) * size, size);
      j--;
    }
    memcpy(b + j * size, t, size);
  }
  free(t);
}

// ---- formatting ----

typedef struct {
  char *out;
  size_t cap, len;
} Sink;

static void put(Sink *k, char c) {
  if (k->len + 1 < k->cap) k->out[k->len] = c;
  k->len++;
}

static void put_n(Sink *k, const char *s, size_t n, int width, int left, char pad) {
  int fill = width > (int)n ? width - (int)n : 0;
  if (!left)
    while (fill-- > 0) put(k, pad);
  // (a sign before zero padding)
  for (size_t i = 0; i < n; i++) put(k, s[i]);
  if (left)
    while (fill-- > 0) put(k, ' ');
}

// x by C's %g: %e or %f by its exponent, trailing zeros dropped.
static int float_g(double x, int prec, char *out, int64_t cap) {
  if (prec == 0) prec = 1;
  char e[400];
  host_float(x, 'e', prec - 1, e, sizeof e);
  const char *ep = strchr(e, 'e');
  int ex = ep ? atoi(ep + 1) : 0;
  int n = ex < -4 || ex >= prec ? host_float(x, 'e', prec - 1, out, cap) : host_float(x, 'f', prec - 1 - ex, out, cap);
  // (the zeros after the point, before an exponent)
  char *pt = strchr(out, '.');
  if (pt) {
    char *xp = strchr(out, 'e');
    char *z = xp ? xp : out + n;
    char *k = z;
    while (k > pt + 1 && k[-1] == '0') k--;
    if (k == pt + 1) k = pt;
    memmove(k, z, (size_t)(out + n - z) + 1);
    n -= (int)(z - k);
  }
  return n;
}

int vsnprintf(char *out, size_t cap, const char *f, va_list ap) {
  Sink k = {out, cap, 0};
  for (; *f; f++) {
    if (*f != '%') {
      put(&k, *f);
      continue;
    }
    f++;
    int left = 0, zero = 0, plus = 0, space = 0, width = 0, prec = -1, lng = 0;
    for (;; f++) {
      if (*f == '-') left = 1;
      else if (*f == '0') zero = 1;
      else if (*f == '+') plus = 1;
      else if (*f == ' ') space = 1;
      else if (*f == '#') ;
      else break;
    }
    if (*f == '*') width = va_arg(ap, int), f++;
    else
      while (*f >= '0' && *f <= '9') width = width * 10 + (*f++ - '0');
    if (*f == '.') {
      f++;
      prec = 0;
      if (*f == '*') prec = va_arg(ap, int), f++;
      else
        while (*f >= '0' && *f <= '9') prec = prec * 10 + (*f++ - '0');
    }
    for (;; f++) {
      if (*f == 'l' || *f == 'j' || *f == 'z' || *f == 't') lng++;
      else if (*f == 'h') ;
      else break;
    }
    char buf[512];
    int n = 0;
    char c = *f;
    if (c == 'd' || c == 'i' || c == 'u' || c == 'x' || c == 'X' || c == 'o' || c == 'p') {
      uint64_t v;
      int neg = 0;
      if (c == 'p') v = (uint64_t)(uintptr_t)va_arg(ap, void *), lng = 2;
      else if (c == 'd' || c == 'i') {
        int64_t s = lng ? va_arg(ap, int64_t) : va_arg(ap, int);
        neg = s < 0;
        v = neg ? (uint64_t)0 - (uint64_t)s : (uint64_t)s;
      } else v = lng ? va_arg(ap, uint64_t) : va_arg(ap, unsigned);
      unsigned base = c == 'x' || c == 'X' || c == 'p' ? 16 : c == 'o' ? 8 : 10;
      const char *dg = c == 'X' ? "0123456789ABCDEF" : "0123456789abcdef";
      char tmp[32];
      int t = 0;
      do tmp[t++] = dg[v % base], v /= base;
      while (v);
      while (t < prec) tmp[t++] = '0';
      if (neg) buf[n++] = '-';
      else if (plus) buf[n++] = '+';
      else if (space) buf[n++] = ' ';
      if (c == 'p') buf[n++] = '0', buf[n++] = 'x';
      int sign = n;
      if (zero && !left && prec < 0)
        while (n + t < width) buf[n++] = '0';
      (void)sign;
      while (t) buf[n++] = tmp[--t];
      put_n(&k, buf, (size_t)n, width, left, ' ');
    } else if (c == 'c') {
      buf[0] = (char)va_arg(ap, int);
      put_n(&k, buf, 1, width, left, ' ');
    } else if (c == 's') {
      const char *s = va_arg(ap, const char *);
      if (!s) s = "(null)";
      size_t m = 0;
      while ((prec < 0 || (int)m < prec) && s[m]) m++;
      put_n(&k, s, m, width, left, ' ');
    } else if (c == 'e' || c == 'E' || c == 'f' || c == 'F' || c == 'g' || c == 'G') {
      double x = va_arg(ap, double);
      if (prec < 0) prec = 6;
      if (plus && !signbit(x)) buf[n++] = '+';
      else if (space && !signbit(x)) buf[n++] = ' ';
      if (c == 'g' || c == 'G') n += float_g(x, prec, buf + n, (int64_t)sizeof buf - n);
      else n += host_float(x, c == 'e' || c == 'E' ? 'e' : 'f', prec, buf + n, (int64_t)sizeof buf - n);
      if (n > (int)sizeof buf - 1) n = (int)sizeof buf - 1;
      if (zero && !left && n < width) {
        // (zeros after the sign)
        int s = buf[0] == '-' || buf[0] == '+' || buf[0] == ' ';
        memmove(buf + s + (width - n), buf + s, (size_t)(n - s));
        memset(buf + s, '0', (size_t)(width - n));
        n = width;
      }
      put_n(&k, buf, (size_t)n, width, left, ' ');
    } else if (c == '%') {
      put(&k, '%');
    } else if (!c) {
      break;
    }
  }
  if (cap) out[k.len < cap ? k.len : cap - 1] = 0;
  return (int)k.len;
}

int snprintf(char *out, size_t n, const char *f, ...) {
  va_list ap;
  va_start(ap, f);
  int r = vsnprintf(out, n, f, ap);
  va_end(ap);
  return r;
}

int sprintf(char *out, const char *f, ...) {
  va_list ap;
  va_start(ap, f);
  int r = vsnprintf(out, (size_t)1 << 30, f, ap);
  va_end(ap);
  return r;
}

// ---- output: to the host ----

struct FILE {
  int fd;
};
static FILE out_file = {1}, err_file = {2};
FILE *stdout = &out_file, *stderr = &err_file;
int errno;

ssize_t write(int fd, const void *p, size_t n) {
  host_write(fd, p, (int64_t)n);
  return (ssize_t)n;
}

int vfprintf(FILE *f, const char *fmt, va_list ap) {
  va_list again;
  va_copy(again, ap);
  int n = vsnprintf(NULL, 0, fmt, again);
  va_end(again);
  char *b = malloc((size_t)n + 1);
  vsnprintf(b, (size_t)n + 1, fmt, ap);
  write(f->fd, b, (size_t)n);
  free(b);
  return n;
}

int fprintf(FILE *f, const char *fmt, ...) {
  va_list ap;
  va_start(ap, fmt);
  int r = vfprintf(f, fmt, ap);
  va_end(ap);
  return r;
}

int printf(const char *fmt, ...) {
  va_list ap;
  va_start(ap, fmt);
  int r = vfprintf(stdout, fmt, ap);
  va_end(ap);
  return r;
}

int fputs(const char *s, FILE *f) { return (int)write(f->fd, s, strlen(s)); }
int fputc(int c, FILE *f) {
  char b = (char)c;
  write(f->fd, &b, 1);
  return c;
}
int puts(const char *s) {
  fputs(s, stdout);
  return fputc('\n', stdout);
}
int putchar(int c) { return fputc(c, stdout); }
size_t fwrite(const void *p, size_t size, size_t n, FILE *f) {
  write(f->fd, p, size * n);
  return n;
}
int fflush(FILE *f) {
  (void)f;
  return 0;
}
char *getenv(const char *name) {
  (void)name;
  return NULL;
}
char *strerror(int e) {
  (void)e;
  return "error";
}

// ---- ending ----

_Noreturn void exit(int code) {
  host_exit(code);
  __builtin_trap();
}
_Noreturn void _exit(int code) { exit(code); }
_Noreturn void abort(void) { exit(134); }
int atexit(void (*f)(void)) {
  (void)f;
  return 0;
}

typedef struct {
  int64_t len;
  char bytes[];
} Str;

// What core.rt's faults ask of the C runtime (runtime.c's versions are left
// out of a module, with its tasks and tests): a fault ends the call, its
// message written to the host's standard error first (`strata_fd_write`),
// then `strata_process_exit`, which the host turns into an exception.
char *strata_task_origin(void) { return NULL; }
void strata_contain_fault(int64_t kind, Str *msg) {
  (void)kind;
  (void)msg;
}
void strata_out_flush(void) {}
void strata_fd_write(int64_t fd, const char *p, int64_t n) { host_write((int32_t)fd, p, n); }
_Noreturn void strata_process_exit(int64_t code) { exit((int)code); }
_Noreturn void strata_thread_park(void) { exit(101); }
_Bool strata_soft_fault(int64_t kind, Str *msg) {
  host_fault((int32_t)kind, msg->bytes, msg->len);
  __builtin_trap();
}
// (no test runs in a module: a run can't be left early)
int64_t strata_trap_run(void *buf, void (*f)(void)) {
  (void)buf;
  f();
  return 0;
}
_Noreturn void strata_trap_leave(void *buf) {
  (void)buf;
  exit(101);
}
int64_t strata_trap_size(void) { return 8; }

// ---- math the instructions lack ----

double round(double x) {
  double t = trunc(x);
  if (fabs(x - t) >= 0.5) t += copysign(1.0, x);
  return t;
}
long long llround(double x) { return (long long)round(x); }
long lround(double x) { return (long)round(x); }

double frexp(double x, int *e) {
  *e = 0;
  if (x == 0 || isnan(x) || isinf(x)) return x;
  union {
    double d;
    uint64_t u;
  } v = {x};
  int ex = (int)((v.u >> 52) & 0x7ff);
  if (ex == 0) {
    // (subnormal: scaled up first)
    v.d *= 0x1p64;
    ex = (int)((v.u >> 52) & 0x7ff) - 64;
  }
  *e = ex - 1022;
  v.u = (v.u & ~((uint64_t)0x7ff << 52)) | ((uint64_t)1022 << 52);
  return v.d;
}

double ldexp(double x, int e) {
  while (e > 1000) x *= 0x1p1000, e -= 1000;
  while (e < -1000) x *= 0x1p-1000, e += 1000;
  union {
    double d;
    uint64_t u;
  } s = {.u = (uint64_t)(e + 1023) << 52};
  return x * s.d;
}

double modf(double x, double *i) {
  *i = trunc(x);
  return isinf(x) ? copysign(0.0, x) : x - *i;
}

// 128-bit multiplication, which LLVM calls for overflow checks
typedef unsigned __int128 u128;
__int128 __multi3(__int128 a, __int128 b) {
  u128 x = (u128)a, y = (u128)b;
  uint64_t xl = (uint64_t)x, xh = (uint64_t)(x >> 64), yl = (uint64_t)y, yh = (uint64_t)(y >> 64);
  uint64_t a0 = xl & 0xffffffff, a1 = xl >> 32, b0 = yl & 0xffffffff, b1 = yl >> 32;
  uint64_t p00 = a0 * b0, p01 = a0 * b1, p10 = a1 * b0, p11 = a1 * b1;
  uint64_t mid = (p00 >> 32) + (p01 & 0xffffffff) + (p10 & 0xffffffff);
  uint64_t lo = (p00 & 0xffffffff) | (mid << 32);
  uint64_t hi = p11 + (p01 >> 32) + (p10 >> 32) + (mid >> 32) + xl * yh + xh * yl;
  return (__int128)(((u128)hi << 64) | lo);
}
