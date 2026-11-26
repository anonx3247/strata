// What compiler/cache asks of the system: where this executable is, and its
// identity, the SHA-256 of its bytes. Every cache key starts with that
// identity, so a cache is valid for the same executable wherever it sits
// (a release ships one beside it, populated on another machine), and a new
// compiler (other bytes) never reads an old one's entries. Hashing the
// executable takes about 0.2 s, so the hash is read from an identity file
// beside it when one still describes it (`cache_self_hash`).

#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <limits.h>
#include <stdlib.h>
#include <unistd.h>
#include <sys/stat.h>
#include <errno.h>
#include <fts.h>
#include <signal.h>
#ifdef __APPLE__
#include <CommonCrypto/CommonDigest.h>
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

#ifdef __APPLE__
#define MTIME(st) ((st).st_mtimespec)
#else
#define MTIME(st) ((st).st_mtim)
#endif

// The SHA-256 of the file at `path`, as 64 hex digits into `out`; 0 when
// it can't be read. (CommonCrypto on macOS: the CPU's SHA instructions,
// several times this file's code on a 100 MB executable.)
static int hash_file(const char *path, char out[65]) {
  FILE *f = fopen(path, "rb");
  if (!f) return 0;
  enum { CHUNK = 1 << 20 };
  unsigned char *buf = malloc(CHUNK);
  if (!buf) {
    fclose(f);
    return 0;
  }
  size_t n;
#ifdef __APPLE__
  CC_SHA256_CTX c;
  CC_SHA256_Init(&c);
  while ((n = fread(buf, 1, CHUNK, f)) > 0) CC_SHA256_Update(&c, buf, (CC_LONG)n);
#else
  sha256_state s;
  sha256_init(&s);
  while ((n = fread(buf, 1, CHUNK, f)) > 0) sha256_update(&s, buf, n);
#endif
  int bad = ferror(f);
  fclose(f);
  free(buf);
  if (bad) return 0;
#ifdef __APPLE__
  unsigned char d[CC_SHA256_DIGEST_LENGTH];
  CC_SHA256_Final(d, &c);
  static const char digits[] = "0123456789abcdef";
  for (int i = 0; i < 32; i++) {
    out[2 * i] = digits[d[i] >> 4];
    out[2 * i + 1] = digits[d[i] & 15];
  }
  out[64] = 0;
#else
  sha256_hex(&s, out);
#endif
  return 1;
}

// `HASH SIZE MTIME` of the file at `path` (its SHA-256, size and
// modification time now) written to `file`, whole or not at all: 0 when
// written.
static int write_identity(const char *file, const char *hash, const struct stat *st) {
  char tmp[PATH_MAX + 64];
  snprintf(tmp, sizeof tmp, "%s.tmp-%d", file, (int)getpid());
  FILE *f = fopen(tmp, "w");
  if (!f) return 1;
  int bad = fprintf(f, "%s %lld %lld\n", hash, (long long)st->st_size, (long long)st->st_mtime) < 0;
  bad |= fclose(f) != 0;
  if (bad || rename(tmp, file) != 0) {
    unlink(tmp);
    return 1;
  }
  return 0;
}

// `<path>.identity` written for the executable at `path`, as
// tools/bootstrap.sh writes it: `strata build` writes one beside a
// compiler it builds (compiler/main's `linked`), so that the compiler's
// first runs read its identity instead of hashing it. 0 when written.
int32_t cache_write_identity(const char *path) {
  struct stat st;
  char hash[65], file[PATH_MAX + 16];
  if (stat(path, &st) != 0 || !hash_file(path, hash)) return 1;
  snprintf(file, sizeof file, "%s.identity", path);
  return write_identity(file, hash, &st);
}

static int is_hex64(const char *h) {
  for (int i = 0; i < 64; i++)
    if (!((h[i] >= '0' && h[i] <= '9') || (h[i] >= 'a' && h[i] <= 'f'))) return 0;
  return 1;
}

// An identity file: `HASH SIZE MTIME` (the executable's SHA-256, its size
// in bytes and its modification time in seconds when HASH was computed).
// Its HASH into `out` when SIZE and MTIME are the executable's now (`st`);
// else 0.
static int read_identity(const char *file, const struct stat *st, char out[65]) {
  FILE *f = fopen(file, "r");
  if (!f) return 0;
  char hash[65];
  long long size, mtime;
  int got = fscanf(f, "%64s %lld %lld", hash, &size, &mtime);
  fclose(f);
  if (got != 3 || strlen(hash) != 64 || !is_hex64(hash)) return 0;
  if (size != (long long)st->st_size || mtime != (long long)st->st_mtime) return 0;
  memcpy(out, hash, 65);
  return 1;
}

// The own cache's folder: $STRATA_CACHE, else ~/.cache/strata (as
// compiler/cache's `caches_of`); 0 when neither is set.
static int own_cache(char *out, size_t n) {
  const char *dir = getenv("STRATA_CACHE");
  if (dir && dir[0]) return snprintf(out, n, "%s", dir) < (int)n;
  const char *home = getenv("HOME");
  if (!home) home = ".";
  return snprintf(out, n, "%s/.cache/strata", home) < (int)n;
}

static void make_dirs(char *path) {
  for (char *p = path + 1; *p; p++) {
    if (*p != '/') continue;
    *p = 0;
    mkdir(path, 0755);
    *p = '/';
  }
  mkdir(path, 0755);
}

// The SHA-256 of this executable's bytes, as 64 hex digits, found once per
// process; "" when the executable can't be read. Hashing all of it (about
// 0.2 s) is the last resort: first an identity file that tools/release.sh
// and tools/bootstrap.sh write beside it (`lib/strata/IDENTITY` in a
// release, `<executable>.identity` for a build), trusted while the
// executable's size and modification time are those it records; then the
// own cache's record for this path, size, modification time and inode
// (`identity/<key>`, written after hashing).
const char *cache_self_hash(void) {
  if (self_hash[0]) return self_hash;
  const char *path = cache_self_path();
  if (!path[0]) return "";
  struct stat st;
  if (stat(path, &st) != 0) return "";
  char file[PATH_MAX + 64];
  // a release: bin/strata and lib/strata/IDENTITY
  const char *slash = strrchr(path, '/');
  if (slash) {
    int dir = (int)(slash - path);
    snprintf(file, sizeof file, "%.*s/../lib/strata/IDENTITY", dir, path);
    if (read_identity(file, &st, self_hash)) return self_hash;
  }
  snprintf(file, sizeof file, "%s.identity", path);
  if (read_identity(file, &st, self_hash)) return self_hash;
  // the own cache's record, keyed by where the executable is and its stat
  char own[PATH_MAX], rec[PATH_MAX + 128], key[65];
  int have_own = own_cache(own, sizeof own);
  if (have_own) {
    char what[PATH_MAX + 128];
    int n = snprintf(what, sizeof what, "%s\n%lld\n%lld.%09ld\n%llu", path, (long long)st.st_size,
                     (long long)MTIME(st).tv_sec, (long)MTIME(st).tv_nsec,
                     (unsigned long long)st.st_ino);
    sha256_state s;
    sha256_init(&s);
    sha256_update(&s, (const unsigned char *)what, (size_t)n);
    sha256_hex(&s, key);
    snprintf(rec, sizeof rec, "%s/identity/%s", own, key);
    if (read_identity(rec, &st, self_hash)) return self_hash;
  }
  if (!hash_file(path, self_hash)) {
    self_hash[0] = 0;
    return "";
  }
  if (have_own) {
    char dir[PATH_MAX + 16];
    snprintf(dir, sizeof dir, "%s/identity", own);
    make_dirs(dir);
    write_identity(rec, self_hash, &st);
  }
  return self_hash;
}

// This process's id (the run's scratch folders are named by it).
int64_t cache_pid(void) { return (int64_t)getpid(); }

/* A new stable id for a definition (compiler/binary's `new_id`): 64 random
   bits, never 0. */
uint64_t binary_new_id(void) {
  uint64_t x;
  do {
    arc4random_buf(&x, sizeof x);
  } while (x == 0);
  return x;
}

// Whether the process `pid` is running (another user's too), for `strata
// cache clean`: a scratch folder named by a running process's id is in use.
int32_t cache_pid_running(int64_t pid) {
  if (pid <= 0) return 0;
  return kill((pid_t)pid, 0) == 0 || errno == EPERM;
}

// The bytes the files and folders under `path` take on disk, as du counts
// them (their blocks; links not followed, other file systems not entered);
// 0 when nothing is there. For `strata cache`.
int64_t cache_tree_bytes(const char *path) {
  char *paths[] = {(char *)path, NULL};
  FTS *f = fts_open(paths, FTS_PHYSICAL | FTS_NOCHDIR | FTS_XDEV, NULL);
  if (!f) return 0;
  int64_t total = 0;
  FTSENT *e;
  while ((e = fts_read(f))) {
    switch (e->fts_info) {
    case FTS_F: case FTS_D: case FTS_SL: case FTS_SLNONE: case FTS_DEFAULT:
      total += (int64_t)e->fts_statp->st_blocks * 512;
      break;
    default:
      break;
    }
  }
  fts_close(f);
  return total;
}

// The hash of the sources this executable was built from: `strata build`
// defines `strata_build_sources` in the program's IR (compiler/main's
// `sources_global`), which takes the place of this weak, empty one; a
// compiler built by one that doesn't keeps "" (then the caches key what it
// makes by its identity).
__attribute__((weak)) const char strata_build_sources[] = "";
const char *cache_build_sources(void) { return strata_build_sources; }

// ---- SHA-256 for compiler/cache and compiler/binary ----
//
// Keys and definitions' hashes, as 64 hex digits: macOS's CommonCrypto
// (CC_SHA256, which uses the CPU's SHA instructions), the code above
// elsewhere. A string is hashed as its bytes; definitions' bytes come as
// hex (a `String` can't hold a NUL for C), two digits a byte.

#ifdef __APPLE__
#include <CommonCrypto/CommonDigest.h>
#endif

static _Thread_local char digest_hex[65];

static const char *digest_of(const unsigned char *p, size_t n) {
#ifdef __APPLE__
  unsigned char d[CC_SHA256_DIGEST_LENGTH];
  CC_SHA256_CTX c;
  CC_SHA256_Init(&c);
  // (CC_LONG is 32 bits: in parts)
  while (n > 0) {
    CC_LONG k = n > (1u << 30) ? (1u << 30) : (CC_LONG)n;
    CC_SHA256_Update(&c, p, k);
    p += k;
    n -= k;
  }
  CC_SHA256_Final(d, &c);
  static const char digits[] = "0123456789abcdef";
  for (int i = 0; i < 32; i++) {
    digest_hex[2 * i] = digits[d[i] >> 4];
    digest_hex[2 * i + 1] = digits[d[i] & 15];
  }
  digest_hex[64] = 0;
#else
  sha256_state s;
  sha256_init(&s);
  sha256_update(&s, p, n);
  sha256_hex(&s, digest_hex);
#endif
  return digest_hex;
}

// The SHA-256 of the string `s`'s bytes.
const char *cache_sha256_text(const char *s) { return digest_of((const unsigned char *)s, strlen(s)); }

static int hex_digit(char c) {
  return c >= '0' && c <= '9' ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 : c >= 'A' && c <= 'F' ? c - 'A' + 10 : 0;
}

// The SHA-256 of the bytes written in hex in `hex` (two digits a byte).
const char *binary_sha256_hex_bytes(const char *hex) {
  size_t n = strlen(hex) / 2;
  unsigned char small[4096];
  unsigned char *b = n <= sizeof small ? small : malloc(n);
  if (!b) return "";
  for (size_t i = 0; i < n; i++) b[i] = (unsigned char)(hex_digit(hex[2 * i]) << 4 | hex_digit(hex[2 * i + 1]));
  const char *r = digest_of(b, n);
  if (b != small) free(b);
  return r;
}

// The SHA-256 of the bytes of the file at `path`, "" when it can't be read.
const char *cache_sha256_file(const char *path) {
  FILE *f = fopen(path, "rb");
  if (!f) return "";
  size_t cap = 1 << 16, n = 0, k;
  unsigned char *b = malloc(cap);
  while (b && (k = fread(b + n, 1, cap - n, f)) > 0) {
    n += k;
    if (n == cap) {
      unsigned char *g = realloc(b, cap *= 2);
      if (!g) free(b);
      b = g;
    }
  }
  int bad = !b || ferror(f);
  fclose(f);
  if (bad) {
    free(b);
    return "";
  }
  const char *r = digest_of(b, n);
  free(b);
  return r;
}
