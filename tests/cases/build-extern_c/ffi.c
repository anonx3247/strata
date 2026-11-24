#include <ctype.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
int8_t neg8(int8_t x) { return (int8_t)-x; }
uint8_t inc8(uint8_t x) { return (uint8_t)(x + 1); }
uint64_t umax(void) { return UINT64_MAX; }
int64_t imin(void) { return INT64_MIN; }
uint64_t half(uint64_t x) { return x / 2; }
bool even(int32_t x) { return x % 2 == 0; }
double scale(double x, bool twice) { return twice ? x * 2 : x; }
const char *shout(const char *s) {
  static char buf[64];
  size_t i = 0;
  for (; s[i] && i < 63; i++) buf[i] = (char)toupper((unsigned char)s[i]);
  buf[i] = 0;
  return buf;
}
typedef struct { int32_t n; } Counter;
int32_t counter_open(int32_t start, Counter **out) {
  if (start < 0) { *out = NULL; return 1; }
  *out = malloc(sizeof **out);
  (*out)->n = start;
  return 0;
}
int32_t counter_bump(Counter *c) { return ++c->n; }
void counter_close(Counter *c) { free(c); }
Counter *nothing(void) { return NULL; }
void twice(int32_t *x) { *x *= 2; }
