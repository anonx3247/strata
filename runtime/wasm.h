// What runtime.c needs of a C library in a WebAssembly module (`strata
// build --target wasm64`), which has none: clang's freestanding headers,
// declarations of the functions runtime/wasm.c provides, and no-op locks
// (a module runs on one thread). Math the instructions lack, and printing,
// come from the host as imports (`env`), which the generated loader gives.
#include <float.h>
#include <limits.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef long ssize_t;
typedef int pid_t;

// memory (wasm.c: size classes over `memory.grow`)
void *malloc(size_t n);
void *calloc(size_t n, size_t k);
void *realloc(void *p, size_t n);
void free(void *p);
size_t malloc_usable_size(void *p);
int posix_memalign(void **out, size_t align, size_t n);

// strings and bytes
void *memcpy(void *d, const void *s, size_t n);
void *memmove(void *d, const void *s, size_t n);
void *memset(void *d, int c, size_t n);
int memcmp(const void *a, const void *b, size_t n);
void *memchr(const void *s, int c, size_t n);
size_t strlen(const char *s);
int strcmp(const char *a, const char *b);
int strncmp(const char *a, const char *b, size_t n);
char *strchr(const char *s, int c);
char *strrchr(const char *s, int c);
char *strstr(const char *h, const char *n);
char *strdup(const char *s);
char *strndup(const char *s, size_t n);
int atoi(const char *s);
long long strtoll(const char *s, char **end, int base);
long strtol(const char *s, char **end, int base);
unsigned long long strtoull(const char *s, char **end, int base);
double strtod(const char *s, char **end);
void qsort(void *base, size_t n, size_t size, int (*cmp)(const void *, const void *));

// formatting (wasm.c: %d %i %u %x %c %s %p with l, ll, z, widths and
// precision; floats through the host's `strata_wasm_float`)
int vsnprintf(char *out, size_t n, const char *fmt, va_list ap);
int snprintf(char *out, size_t n, const char *fmt, ...);
int sprintf(char *out, const char *fmt, ...);

// output: to the host (`strata_wasm_write`)
typedef struct FILE FILE;
extern FILE *stdout, *stderr;
int fprintf(FILE *f, const char *fmt, ...);
int vfprintf(FILE *f, const char *fmt, va_list ap);
int printf(const char *fmt, ...);
int fputs(const char *s, FILE *f);
int fputc(int c, FILE *f);
int puts(const char *s);
int putchar(int c);
size_t fwrite(const void *p, size_t size, size_t n, FILE *f);
int fflush(FILE *f);
ssize_t write(int fd, const void *p, size_t n);
char *getenv(const char *name);
char *strerror(int e);
extern int errno;
#define EINTR 4
#define ENOMEM 12

// ending: the host is told why, then the module traps
_Noreturn void abort(void);
_Noreturn void exit(int code);
_Noreturn void _exit(int code);
int atexit(void (*f)(void));

// one thread: locks do nothing
typedef int pthread_mutex_t;
typedef int pthread_cond_t;
typedef int pthread_t;
#define PTHREAD_MUTEX_INITIALIZER 0
#define PTHREAD_COND_INITIALIZER 0
static inline int pthread_mutex_lock(pthread_mutex_t *m) { (void)m; return 0; }
static inline int pthread_mutex_unlock(pthread_mutex_t *m) { (void)m; return 0; }
static inline int pthread_mutex_init(pthread_mutex_t *m, void *a) { (void)m; (void)a; return 0; }
static inline int pthread_cond_signal(pthread_cond_t *c) { (void)c; return 0; }
static inline int pthread_cond_broadcast(pthread_cond_t *c) { (void)c; return 0; }

// math: instructions where WebAssembly has them, else the host's (`env`)
#ifndef INFINITY
#define INFINITY __builtin_inff()
#endif
#ifndef NAN
#define NAN __builtin_nanf("")
#endif
#define HUGE_VAL __builtin_huge_val()
#define isnan(x) __builtin_isnan(x)
#define isinf(x) __builtin_isinf(x)
#define isfinite(x) __builtin_isfinite(x)
#define signbit(x) __builtin_signbit(x)
#define sqrt __builtin_sqrt
#define fabs __builtin_fabs
#define floor __builtin_floor
#define ceil __builtin_ceil
#define trunc __builtin_trunc
#define nearbyint __builtin_nearbyint
#define rint __builtin_rint
#define copysign __builtin_copysign
#define fmin __builtin_fmin
#define fmax __builtin_fmax
double round(double x);
long long llround(double x);
long lround(double x);
double fmod(double a, double b);
double remainder(double a, double b);
double pow(double a, double b);
double exp(double x);
double exp2(double x);
double expm1(double x);
double log(double x);
double log2(double x);
double log10(double x);
double log1p(double x);
double sin(double x);
double cos(double x);
double tan(double x);
double asin(double x);
double acos(double x);
double atan(double x);
double atan2(double y, double x);
double sinh(double x);
double cosh(double x);
double tanh(double x);
double cbrt(double x);
double hypot(double a, double b);
double frexp(double x, int *e);
double ldexp(double x, int e);
double modf(double x, double *i);
double fma(double a, double b, double c);

// faults jump nowhere: the module traps
typedef int sigjmp_buf[1];
static inline void flockfile(FILE *f) { (void)f; }
static inline void funlockfile(FILE *f) { (void)f; }
