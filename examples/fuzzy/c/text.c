// C reused by examples/fuzzy through `extern c` (text.strata).
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

// The edit distance between two strings (bytes): the fewest insertions,
// deletions and substitutions that turn one into the other.
int32_t edit_distance(const char *a, const char *b) {
  size_t n = strlen(a), m = strlen(b);
  int32_t *row = malloc((m + 1) * sizeof *row);
  for (size_t j = 0; j <= m; j++) row[j] = (int32_t)j;
  for (size_t i = 1; i <= n; i++) {
    int32_t diag = row[0];
    row[0] = (int32_t)i;
    for (size_t j = 1; j <= m; j++) {
      int32_t up = row[j];
      int32_t best = diag + (a[i - 1] != b[j - 1]);
      if (up + 1 < best) best = up + 1;
      if (row[j - 1] + 1 < best) best = row[j - 1] + 1;
      row[j] = best;
      diag = up;
    }
  }
  int32_t d = row[m];
  free(row);
  return d;
}

// The CRC-32 of a string (as zlib's `crc32`).
uint32_t crc32_of(const char *s) {
  uint32_t c = 0xFFFFFFFFu;
  for (; *s; s++) {
    c ^= (uint8_t)*s;
    for (int k = 0; k < 8; k++) c = (c >> 1) ^ (0xEDB88320u & (0u - (c & 1)));
  }
  return ~c;
}
