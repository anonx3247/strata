// The native runtime of compiled Strata programs: strings, printing, number
// formatting and faults. Linked with the LLVM IR that `strata build` emits.
// It mirrors the interpreter (src/interp.rs): the same output, the same fault
// messages. Memory is never freed yet (no garbage collector).

#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

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

// ---- output ----

void strata_print(Str *s) {
  fwrite(s->bytes, 1, (size_t)s->len, stdout);
  fputc('\n', stdout);
}
