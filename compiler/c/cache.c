// What compiler/cache asks of the system: where this executable is, and its
// identity, the SHA-256 of its bytes. Every cache key starts with that
// identity, so a cache is valid for the same executable wherever it sits
// (a release ships one beside it, populated on another machine), and a new
// compiler (other bytes) never reads an old one's entries.

#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <limits.h>
#include <stdlib.h>
#include <unistd.h>
#ifdef __APPLE__
#include <mach-o/dyld.h>
#endif

static char self_path[PATH_MAX];

// This executable's path, its links followed (a `strata` linked from
// another folder finds the release beside the real one); "" when it can't
// be told.
const char *cache_self_path(void) {
  if (self_path[0]) return self_path;
  char path[PATH_MAX];
#ifdef __APPLE__
  uint32_t n = sizeof path;
  if (_NSGetExecutablePath(path, &n) != 0) return "";
#else
  ssize_t n = readlink("/proc/self/exe", path, sizeof path - 1);
  if (n <= 0) return "";
  path[n] = 0;
#endif
  if (!realpath(path, self_path)) self_path[0] = 0;
  return self_path;
}

// ---- SHA-256 (FIPS 180-4) ----

typedef struct {
  uint32_t h[8];
  uint64_t bits;
  unsigned char block[64];
  size_t used;
} sha256_state;

static const uint32_t K[64] = {
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
};

#define ROTR(x, n) (((x) >> (n)) | ((x) << (32 - (n))))

static void sha256_block(sha256_state *s, const unsigned char *p) {
  uint32_t w[64];
  for (int t = 0; t < 16; t++)
    w[t] = (uint32_t)p[4 * t] << 24 | (uint32_t)p[4 * t + 1] << 16 | (uint32_t)p[4 * t + 2] << 8 | p[4 * t + 3];
  for (int t = 16; t < 64; t++) {
    uint32_t s0 = ROTR(w[t - 15], 7) ^ ROTR(w[t - 15], 18) ^ (w[t - 15] >> 3);
    uint32_t s1 = ROTR(w[t - 2], 17) ^ ROTR(w[t - 2], 19) ^ (w[t - 2] >> 10);
    w[t] = w[t - 16] + s0 + w[t - 7] + s1;
  }
  uint32_t a = s->h[0], b = s->h[1], c = s->h[2], d = s->h[3], e = s->h[4], f = s->h[5], g = s->h[6], h = s->h[7];
  for (int t = 0; t < 64; t++) {
    uint32_t t1 = h + (ROTR(e, 6) ^ ROTR(e, 11) ^ ROTR(e, 25)) + ((e & f) ^ (~e & g)) + K[t] + w[t];
    uint32_t t2 = (ROTR(a, 2) ^ ROTR(a, 13) ^ ROTR(a, 22)) + ((a & b) ^ (a & c) ^ (b & c));
    h = g; g = f; f = e; e = d + t1; d = c; c = b; b = a; a = t1 + t2;
  }
  s->h[0] += a; s->h[1] += b; s->h[2] += c; s->h[3] += d;
  s->h[4] += e; s->h[5] += f; s->h[6] += g; s->h[7] += h;
}

static void sha256_init(sha256_state *s) {
  static const uint32_t h0[8] = {0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                                 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19};
  memcpy(s->h, h0, sizeof h0);
  s->bits = 0;
  s->used = 0;
}

static void sha256_update(sha256_state *s, const unsigned char *p, size_t n) {
  s->bits += (uint64_t)n * 8;
  while (n > 0) {
    if (s->used == 0 && n >= 64) {
      sha256_block(s, p);
      p += 64;
      n -= 64;
      continue;
    }
    size_t k = 64 - s->used < n ? 64 - s->used : n;
    memcpy(s->block + s->used, p, k);
    s->used += k;
    p += k;
    n -= k;
    if (s->used == 64) {
      sha256_block(s, s->block);
      s->used = 0;
    }
  }
}

static void sha256_hex(sha256_state *s, char out[65]) {
  uint64_t bits = s->bits;
  unsigned char pad = 0x80, zero = 0, len[8];
  sha256_update(s, &pad, 1);
  while (s->used != 56) sha256_update(s, &zero, 1);
  for (int i = 0; i < 8; i++) len[i] = (unsigned char)(bits >> (56 - 8 * i));
  sha256_update(s, len, 8);
  for (int i = 0; i < 8; i++) snprintf(out + 8 * i, 9, "%08x", s->h[i]);
}

static char self_hash[65];

// The SHA-256 of this executable's bytes, as 64 hex digits, computed once
// per process; "" when the executable can't be read.
const char *cache_self_hash(void) {
  if (self_hash[0]) return self_hash;
  const char *path = cache_self_path();
  FILE *f = path[0] ? fopen(path, "rb") : NULL;
  if (!f) return "";
  sha256_state s;
  sha256_init(&s);
  static unsigned char buf[1 << 20];
  size_t n;
  while ((n = fread(buf, 1, sizeof buf, f)) > 0) sha256_update(&s, buf, n);
  int bad = ferror(f);
  fclose(f);
  if (bad) return "";
  sha256_hex(&s, self_hash);
  return self_hash;
}
