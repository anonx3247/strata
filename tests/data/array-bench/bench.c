// The C side of bench.st: `clang -O2 bench.c -o bench && ./bench`.
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

static double now(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e6 + t.tv_nsec / 1e3;
}

int main(void) {
  long n = 10000000;
  double *x = malloc(n * 8);
  for (long i = 0; i < n; i++) x[i] = (double)i;
  double t0 = now(), s = 0;
  for (int r = 0; r < 10; r++) {
    double t = 0;
    for (long i = 0; i < n; i++) t += x[i];
    s += t;
  }
  printf("sum, one accumulator %.1f: %.0f us per 10M\n", s, (now() - t0) / 10);
  t0 = now(), s = 0;
  for (int r = 0; r < 10; r++) {
    double a = 0, b = 0, c = 0, d = 0;
    long i = 0;
    for (; i + 3 < n; i += 4) { a += x[i]; b += x[i + 1]; c += x[i + 2]; d += x[i + 3]; }
    for (; i < n; i++) a += x[i];
    s += (a + b) + (c + d);
  }
  printf("sum, four accumulators %.1f: %.0f us per 10M\n", s, (now() - t0) / 10);

  int N = 256;
  double *A = malloc(N * N * 8), *B = malloc(N * N * 8), *BT = malloc(N * N * 8), *C = calloc(N * N, 8);
  for (int i = 0; i < N * N; i++) { A[i] = (i % 5) * 0.25; B[i] = (i % 3) * 0.5; }
  t0 = now();
  for (int r = 0; r < 10; r++) {
    for (int i = 0; i < N; i++) for (int j = 0; j < N; j++) BT[j * N + i] = B[i * N + j];
    for (int i = 0; i < N; i++) for (int j = 0; j < N; j++) {
      double t = 0;
      for (int k = 0; k < N; k++) t += A[i * N + k] * BT[j * N + k];
      C[i * N + j] = t;
    }
  }
  double cs = 0;
  for (int i = 0; i < N * N; i++) cs += C[i];
  printf("matmul, dot form %.2f: %.0f us\n", cs, (now() - t0) / 10);
  for (int i = 0; i < N * N; i++) C[i] = 0;
  t0 = now();
  for (int r = 0; r < 10; r++) {
    for (int i = 0; i < N * N; i++) C[i] = 0;
    for (int i = 0; i < N; i++) for (int k = 0; k < N; k++) {
      double aik = A[i * N + k];
      for (int j = 0; j < N; j++) C[i * N + j] += aik * B[k * N + j];
    }
  }
  cs = 0;
  for (int i = 0; i < N * N; i++) cs += C[i];
  printf("matmul, i-k-j %.2f: %.0f us\n", cs, (now() - t0) / 10);
}
