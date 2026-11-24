// The compiler's symbols (compiler/ast's `Sym`): a name as a number, so
// that names are compared, ordered and used as keys as integers. A name's
// number is a 61-bit hash of its text, the same in every run whatever order
// names are met in (so that a table ordered by symbol is ordered the same way
// in every build: the bootstrap needs the same bytes twice, and threads may
// meet names in any order). The texts are kept here, one table for the whole
// process, never freed. Two texts with the same hash would be one symbol:
// interning refuses that (a fault), it doesn't happen silently (at 2^61, one
// chance in billions for a hundred thousand names).
//
// Safe to call from any thread: the table is behind a lock.

#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// Open addressing on the numbers; a slot's text is NULL when it is empty.
typedef struct {
  int64_t sym;
  char *text;
} Slot;

static Slot *slots;
static int64_t capacity, count;
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;

static void *must(void *p) {
  if (!p) {
    fputs("fault (Limit): out of memory\n", stderr);
    exit(101);
  }
  return p;
}

// FNV-1a, then mixed (splitmix64's finalizer), kept to 61 bits: an `Int`
// below 2^62 is a machine integer in Strata, never a big one.
static int64_t hash_of(const char *s, size_t n) {
  uint64_t h = 1469598103934665603ULL;
  for (size_t i = 0; i < n; i++) {
    h ^= (unsigned char)s[i];
    h *= 1099511628211ULL;
  }
  h ^= h >> 30;
  h *= 0xbf58476d1ce4e5b9ULL;
  h ^= h >> 27;
  h *= 0x94d049bb133111ebULL;
  h ^= h >> 31;
  return (int64_t)(h & ((1ULL << 61) - 1));
}

static Slot *slot_of(int64_t sym) {
  uint64_t j = (uint64_t)sym & (uint64_t)(capacity - 1);
  while (slots[j].text && slots[j].sym != sym) j = (j + 1) & (uint64_t)(capacity - 1);
  return &slots[j];
}

static void grow(void) {
  int64_t old = capacity;
  Slot *was = slots;
  capacity = old ? old * 2 : 1 << 14;
  slots = must(calloc((size_t)capacity, sizeof *slots));
  for (int64_t i = 0; i < old; i++)
    if (was[i].text) *slot_of(was[i].sym) = was[i];
  free(was);
}

// The symbol of `s` (NUL-terminated), its text kept the first time.
int64_t sym_intern(const char *s) {
  size_t n = strlen(s);
  int64_t sym = hash_of(s, n);
  pthread_mutex_lock(&lock);
  if ((count + 1) * 2 > capacity) grow();
  Slot *at = slot_of(sym);
  if (!at->text) {
    at->sym = sym;
    at->text = must(malloc(n + 1));
    memcpy(at->text, s, n + 1);
    count++;
  } else if (strcmp(at->text, s) != 0) {
    fprintf(stderr, "fault (Limit): the names `%s` and `%s` have the same symbol\n", at->text, s);
    exit(101);
  }
  pthread_mutex_unlock(&lock);
  return sym;
}

// The text of symbol `sym`; "" for one never given.
const char *sym_text(int64_t sym) {
  pthread_mutex_lock(&lock);
  const char *t = capacity ? slot_of(sym)->text : NULL;
  pthread_mutex_unlock(&lock);
  return t ? t : "";
}
