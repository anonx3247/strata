// The C side of bench.st: `clang -O2 bench.c -o bench && ./bench`. The same
// algorithms as std.array (four partial sums; matmul as one dot product per
// element over b's transpose), and the vectorized i-k-j matmul for float64.
// Each line: the best of 5 runs, in microseconds.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

static double now(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e6 + t.tv_nsec / 1e3;
}

#define BEST(var, ...) do { double b_ = 1e18; for (int r_ = 0; r_ < 5; r_++) { double t_ = now(); __VA_ARGS__; double d_ = now() - t_; if (d_ < b_) b_ = d_; } var = b_; } while (0)

#define BENCH(T, S, NAME) static void bench_##NAME(void) { \
  long n = 10000000; T *x = malloc(n * sizeof(T)), *y = malloc(n * sizeof(T)); \
  for (long i = 0; i < n; i++) x[i] = (T)i; \
  volatile S sink; double t; \
  BEST(t, { S a = 0, b = 0, c = 0, d = 0; long i = 0; \
    for (; i + 3 < n; i += 4) { a += x[i]; b += x[i + 1]; c += x[i + 2]; d += x[i + 3]; } \
    for (; i < n; i++) a += x[i]; sink = (a + b) + (c + d); }); \
  printf(#NAME " sum %.0f us\n", t); \
  BEST(t, { for (long i = 0; i < n; i++) y[i] = x[i] + x[i]; sink = y[n - 1]; }); \
  printf(#NAME " add %.0f us\n", t); \
  int N = 256; T *A = malloc(N * N * sizeof(T)), *B = malloc(N * N * sizeof(T)), *BT = malloc(N * N * sizeof(T)), *C = malloc(N * N * sizeof(T)); \
  for (int i = 0; i < N * N; i++) { A[i] = (T)(i % 5); B[i] = (T)(i % 3); } \
  BEST(t, { for (int i = 0; i < N; i++) for (int j = 0; j < N; j++) BT[j * N + i] = B[i * N + j]; \
    for (int i = 0; i < N; i++) for (int j = 0; j < N; j++) { T s = 0; for (int k = 0; k < N; k++) s += A[i * N + k] * BT[j * N + k]; C[i * N + j] = s; } \
    sink = C[0]; }); \
  printf(#NAME " matmul, dot form %.0f us\n", t); \
  BEST(t, { for (int i = 0; i < N * N; i++) C[i] = 0; \
    for (int i = 0; i < N; i++) for (int k = 0; k < N; k++) { T aik = A[i * N + k]; for (int j = 0; j < N; j++) C[i * N + j] += aik * B[k * N + j]; } \
    sink = C[0]; }); \
  printf(#NAME " matmul, i-k-j %.0f us\n", t); \
}

BENCH(double, double, float64)
BENCH(int64_t, int64_t, int64)
BENCH(int32_t, int64_t, int32)

int main(void) {
  bench_float64();
  bench_int64();
  bench_int32();
}
