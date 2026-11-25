// The native runtime of compiled Strata programs: strings, printing, number
// formatting and faults. Linked with the LLVM IR that `strata build` emits.
// It mirrors the interpreter (src/interp.rs): the same output, the same fault
// messages.
//
// Memory is reference-counted (Perceus, as in Koka and Lean): every object
// has a count word just before it, so the layouts below start at the
// object's pointer as if the count weren't there. Constants in the IR have a
// count of 0, which never changes: they are never freed. The generated code
// does the counting (`strata_dup`, `strata_release`, then its own drop of
// the object's contents and `strata_free`); objects never form cycles,
// since values never change once built.

#ifdef __linux__
#define _GNU_SOURCE  // pthread_getattr_np
#endif
#include <dirent.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <errno.h>
#include <float.h>
#include <math.h>
#include <setjmp.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <spawn.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

// A string: its length in bytes, then its UTF-8 bytes (not NUL-terminated).
// String literals are laid out the same way as constants in the IR.
typedef struct {
  int64_t len;
  char bytes[];
} Str;

// Fault kinds, in the order of the IR's constants (`FaultKind` in interp.rs).
static const char *KINDS[] = {"Refinement", "Requires", "Ensures", "Assert", "Runtime", "Hole", "Limit"};

// ---- memory ----

#ifdef __APPLE__
#include <malloc/malloc.h>
#define usable_size malloc_size
#else
#include <malloc.h>
#define usable_size malloc_usable_size
#endif

#ifndef STRATA_CORE_RT
static int64_t allocated, freed;
static pthread_mutex_t made_lock = PTHREAD_MUTEX_INITIALIZER;
#endif

// While a task runs (`spawn`), threads may hold the same objects, so counts
// change atomically; with none alive, they don't. Either way a count of 1
// means only this thread can see the object. Set before the first task's
// thread starts and cleared after the last one is joined, so it never
// changes while another thread reads it.
static int threaded;
static int live_tasks;
#define THREADED __atomic_load_n(&threaded, __ATOMIC_RELAXED)

// With STRATA_MEMORY=check, freed objects are kept, filled with garbage and
// marked, so that touching one again stops the program instead of reading
// whatever reuses the memory.
static _Bool checking;
#define FREED INT64_MIN

// Whether counts must go through `strata_dup` / `strata_release`: while
// tasks run (atomic counts) or in check mode. The compiled code reads it and
// otherwise updates counts itself (`strata.dup`, `strata.release` in the IR).
int32_t strata_slow_counts;

void strata_memory_threaded(_Bool on);

static void set_threaded(int v) {
  __atomic_store_n(&threaded, v, __ATOMIC_RELAXED);
  __atomic_store_n(&strata_slow_counts, v || checking, __ATOMIC_RELAXED);
#ifdef STRATA_CORE_RT
  strata_memory_threaded(v);
#endif
}

// The count word of an object.
static int64_t *count_of(void *p) { return (int64_t *)p - 1; }

// A raw block (core.mem's `raw_alloc`): not counted, not cleared; out of
// memory ends the program, as for any allocation.
void *strata_raw_alloc(int64_t n) {
  void *p = malloc(n > 0 ? (size_t)n : 1);
  if (!p) {
    fputs("fault (Limit): out of memory\n", stderr);
    exit(101);
  }
  return p;
}

// Stops the program (core.mem's `raw_fail`): memory used wrongly.
_Noreturn void strata_raw_fail(Str *m) {
  fflush(stdout);
  fprintf(stderr, "%.*s\n", (int)m->len, m->bytes);
  abort();
}

// How many bytes a raw block holds (core.mem's `raw_size`).
int64_t strata_raw_size(void *p) { return (int64_t)usable_size(p); }

// A reserved range of addresses, zeroed, paged in as it is touched (core.mem's
// `raw_reserve`); NULL when the system refuses it.
void *strata_raw_reserve(int64_t n) {
  void *r = n > 0 ? mmap(NULL, (size_t)n, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0) : MAP_FAILED;
  return r == MAP_FAILED ? NULL : r;
}

// The runtime written in Strata (core/rt.strata) replaces these when it is
// linked in (`STRATA_CORE_RT`: every executable, and strata itself from the
// second stage of its build on); without it, as when strata is first built,
// they are C.
#ifndef STRATA_CORE_RT
// In check mode, every object made (to list those still alive at the end).
static int64_t **made;
static int64_t nmade, capmade;

static _Noreturn void freed_again(const char *what) {
  fflush(stdout);
  fprintf(stderr, "memory: %s of a freed object\n", what);
  abort();
}

// Small objects (with their count word, at most OBJECT_MAX bytes: most are
// Options, tuples, records, list headers and short strings) come from free
// lists, one per size class (16-byte steps), carved from 64 KB chunks of
// one reserved range of addresses; `strata_free` knows them by their
// address, and each chunk's class from `chunk_class`. Much cheaper than
// malloc and free for objects made and dropped all the time. The lists are
// shared by all threads, behind a spinlock (one atomic exchange when no
// other thread wants it). Bigger objects, and every object in check mode
// (whose freed objects are kept and marked), are malloc's.
#define OBJECT_MAX 256
#define CLASSES (OBJECT_MAX / 16 + 1)
#define CHUNK ((size_t)1 << 16)
#define REGION ((size_t)1 << 34)
static char *region, *region_next;
static uint8_t chunk_class[REGION / CHUNK];
static void *free_list[CLASSES];
static int small_lock;

static void small_lock_take(void) {
  while (__atomic_exchange_n(&small_lock, 1, __ATOMIC_ACQUIRE))
    while (__atomic_load_n(&small_lock, __ATOMIC_RELAXED)) {}
}

static void small_lock_give(void) { __atomic_store_n(&small_lock, 0, __ATOMIC_RELEASE); }

static int in_region(void *c) { return region && (char *)c >= region && (char *)c < region + REGION; }

// A block of class `k` (16 * k bytes), or NULL when the region is used up
// (or couldn't be reserved): malloc's then.
static int64_t *small_alloc(int k) {
  small_lock_take();
  void *b = free_list[k];
  if (!b) {
    if (!region) {
      void *r = mmap(NULL, REGION, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0);
      region_next = region = r == MAP_FAILED ? NULL : r;
    }
    if (!region || region_next + CHUNK > region + REGION) {
      small_lock_give();
      return NULL;
    }
    char *chunk = region_next;
    region_next += CHUNK;
    chunk_class[(size_t)(chunk - region) / CHUNK] = (uint8_t)k;
    size_t size = (size_t)k * 16;
    // the chunk's blocks, linked through their first word
    for (char *q = chunk + CHUNK - size - (CHUNK % size); q >= chunk; q -= size) {
      *(void **)q = b;
      b = q;
    }
  }
  free_list[k] = *(void **)b;
  small_lock_give();
  return b;
}

static void small_free(int64_t *c) {
  int k = chunk_class[(size_t)((char *)c - region) / CHUNK];
  small_lock_take();
  *(void **)c = free_list[k];
  free_list[k] = c;
  small_lock_give();
}

// A new object of `n` bytes, held once.
void *strata_alloc(int64_t n) {
  int64_t *c = NULL;
  if (!checking && n + 8 <= OBJECT_MAX) c = small_alloc((int)((n + 8 + 15) / 16));
  if (!c) c = malloc((size_t)n + 8);
  if (!c) {
    fputs("fault (Limit): out of memory\n", stderr);
    exit(101);
  }
  *c = 1;
  int th = THREADED;
  if (th) __atomic_fetch_add(&allocated, 1, __ATOMIC_RELAXED);
  else allocated++;
  if (checking) {
    if (th) pthread_mutex_lock(&made_lock);
    if (nmade == capmade) made = realloc(made, (size_t)(capmade = capmade ? capmade * 2 : 1024) * sizeof *made);
    made[nmade++] = c;
    if (th) pthread_mutex_unlock(&made_lock);
  }
  return c + 1;
}

// One more holder of `p`.
void strata_dup(void *p) {
  int64_t *c = count_of(p);
  if (THREADED) {
    int64_t v = __atomic_load_n(c, __ATOMIC_RELAXED);
    if (v > 0) __atomic_fetch_add(c, 1, __ATOMIC_RELAXED);
    else if (v == FREED) freed_again("dup");
    return;
  }
  if (*c > 0) ++*c;
  else if (*c == FREED) freed_again("dup");
}

// One holder fewer: whether that was the last one, in which case the caller
// drops what the object holds and frees it.
_Bool strata_release(void *p) {
  int64_t *c = count_of(p);
  if (THREADED) {
    int64_t v = __atomic_load_n(c, __ATOMIC_RELAXED);
    if (v == FREED) freed_again("drop");
    return v > 0 && __atomic_sub_fetch(c, 1, __ATOMIC_ACQ_REL) == 0;
  }
  if (*c == FREED) freed_again("drop");
  return *c > 0 && --*c == 0;
}

// Give an object's memory back (its contents already dropped).
void strata_free(void *p) {
  if (THREADED) __atomic_fetch_add(&freed, 1, __ATOMIC_RELAXED);
  else freed++;
  int64_t *c = count_of(p);
  if (in_region(c)) {
    small_free(c);
    return;
  }
  if (checking) {
    memset(p, 0xDB, usable_size(c) - 8);
    *c = FREED;
    return;
  }
  free(c);
}

// Whether `p` has a single holder, so it may be changed in place.
_Bool strata_unique(void *p) { return THREADED ? __atomic_load_n(count_of(p), __ATOMIC_ACQUIRE) == 1 : *count_of(p) == 1; }
#else
void *strata_alloc(int64_t n);
void strata_dup(void *p);
_Bool strata_release(void *p);
void strata_free(void *p);
_Bool strata_unique(void *p);
// core.rt's tallies and the objects made in check mode (count words)
int64_t strata_memory_allocated(void);
int64_t strata_memory_freed(void);
int64_t *strata_memory_made(int64_t i);
void strata_memory_start(_Bool check);
void strata_memory_threaded(_Bool on);
void strata_memory_thread_end(void);
void strata_memory_spawning(void);
#endif

// A string has nothing in it to drop.
static void str_drop(void *s) {
  if (strata_release(s)) strata_free(s);
}

// With STRATA_MEMORY set, how many objects the program made and freed.
// In check mode, the first few still alive: their count, size and first
// word (a block's tag, a string's or a list's length), as text if it reads
// as a string.
static void memory_stats(void) {
#ifdef STRATA_CORE_RT
  int64_t allocated = strata_memory_allocated(), freed = strata_memory_freed();
#endif
  fprintf(stderr, "memory: %lld allocated, %lld freed, %lld live\n", (long long)allocated, (long long)freed, (long long)(allocated - freed));
  int shown = 0;
#ifdef STRATA_CORE_RT
  for (int64_t i = 0, *c; (c = strata_memory_made(i)) && shown < 20; i++) {
#else
  for (int64_t i = 0; i < nmade && shown < 20; i++) {
    int64_t *c = made[i];
#endif
    if (*c == FREED) continue;
    shown++;
    size_t size = usable_size(c) - 8;
    int64_t first = c[1];
    fprintf(stderr, "  live: count %lld, %zu bytes, first word %lld", (long long)*c, size, (long long)first);
    if (first >= 0 && (size_t)first + 8 <= size) {
      // as text, one line: anything but printable ASCII as `.`
      const unsigned char *t = (const unsigned char *)(c + 2);
      fputs(" \"", stderr);
      for (int64_t k = 0; k < first && k < 40; k++) fputc(t[k] >= 32 && t[k] < 127 ? t[k] : '.', stderr);
      fputc('"', stderr);
    }
    fputc('\n', stderr);
  }
}

static Str *str_alloc(int64_t len) {
  Str *s = strata_alloc((int64_t)sizeof(Str) + len);
  s->len = len;
  return s;
}

static Str *str_from(const char *c, int64_t len) {
  Str *s = str_alloc(len);
  memcpy(s->bytes, c, (size_t)len);
  return s;
}

static Str *str_c(const char *c) { return str_from(c, (int64_t)strlen(c)); }

#ifndef STRATA_CORE_RT
Str *strata_concat(Str *a, Str *b) {
  Str *s = str_alloc(a->len + b->len);
  memcpy(s->bytes, a->bytes, (size_t)a->len);
  memcpy(s->bytes + a->len, b->bytes, (size_t)b->len);
  return s;
}

// `a` then `b`, which the caller hands over (both are dropped).
Str *strata_concat_owned(Str *a, Str *b) {
  Str *s = strata_concat(a, b);
  str_drop(a);
  str_drop(b);
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
#else
// (core.rt's: the string functions, below too)
Str *strata_concat(Str *a, Str *b);
Str *strata_concat_owned(Str *a, Str *b);
int64_t strata_str_len(Str *s);
int64_t strata_str_cmp(Str *a, Str *b);
Str *strata_str_trim(Str *s);
#endif

// ---- faults ----

// The functions being run, outermost first (as the interpreter's call
// stack): a fault shows them. A recursion is one run of the same function
// (`down (95846 times)`), so a deep one costs nothing to keep; only the
// first FRAMES runs are kept.
#define FRAMES 4096
// The call stack (`runs` in use, counting those past FRAMES), and the
// state of the thread's run (a test, a fuzz case) and its faults: core.fault's
// and core.test's `builtin thread static` blocks when core.rt is linked in
// (`STRATA_CORE_RT`, laid out as these), this file's own thread-locals in
// the first stage of strata's build.
typedef struct {
  int64_t runs;
  Str *frames[FRAMES];
  int64_t counts[FRAMES];
} CallStack;

typedef struct {
  int64_t started;              // (core.test's: 1 once `rng` and `test_seed` start at 7)
  uint64_t rng;                 // the generators' state
  uint64_t test_seed;           // where a test's numbers start
  sigjmp_buf *trap;             // where a fault goes back to (a test, a task)
  int64_t fault_kind;           // the last caught fault: its kind, message, stack
  Str *fault_msg;
  char *fault_stack;
  char *printed;                // what a test printed
  size_t printed_len, printed_cap;
  int64_t capturing;            // whether a test's output is kept
  int64_t steps, counting;      // a fuzz case's step budget
  char *fuzz_args;              // a fuzz case's arguments as code
  size_t fuzz_args_len;
  int64_t *given;               // values the host wrote for compiled code
  int64_t ct_kept;              // compile-time code's value
  int64_t trace_depth;          // `--trace`'s indentation
} RunState;

#ifdef STRATA_CORE_RT
CallStack *strata_call_stack(void);
RunState *strata_run_state(void);
#define CALLS (*strata_call_stack())
#define RS strata_run_state()
#else
static _Thread_local CallStack calls_c;
static _Thread_local RunState run_c = {1, 7, 7};
#define CALLS calls_c
#define RS (&run_c)
#endif

#ifndef STRATA_CORE_RT
void strata_enter(Str *name) {
  __typeof__(CALLS) *c = &CALLS;
  int64_t r = c->runs;
  if (r > 0 && r <= FRAMES && c->frames[r - 1] == name) {
    c->counts[r - 1]++;
    return;
  }
  if (r < FRAMES) {
    c->frames[r] = name;
    c->counts[r] = 1;
  }
  c->runs = r + 1;
}

void strata_leave(void) {
  __typeof__(CALLS) *c = &CALLS;
  int64_t r = c->runs;
  if (r <= FRAMES && --c->counts[r - 1] > 0) return;
  c->runs = r - 1;
}

// `--trace` (compiler/codegen's): each traced call and return on stderr,
// indented by depth, its values shown by the compiled code:
// `→ total([1, 2])`, then `← total: 3`.

void strata_trace_enter(Str *name, Str *args) {
  for (int64_t i = 0; i < RS->trace_depth; i++) fputs("  ", stderr);
  fprintf(stderr, "\xe2\x86\x92 %.*s(%.*s)\n", (int)name->len, name->bytes, (int)args->len, args->bytes);
  RS->trace_depth++;
}

void strata_trace_leave(Str *name, Str *value) {
  if (RS->trace_depth > 0) RS->trace_depth--;
  for (int64_t i = 0; i < RS->trace_depth; i++) fputs("  ", stderr);
  if (value)
    fprintf(stderr, "\xe2\x86\x90 %.*s: %.*s\n", (int)name->len, name->bytes, (int)value->len, value->bytes);
  else
    fprintf(stderr, "\xe2\x86\x90 %.*s\n", (int)name->len, name->bytes);
}

#ifndef STRATA_CORE_RT
#endif

// The call stack as `strata run` shows it ("" when empty): as the
// `names::show_stack`, four or more calls in a row of one function
// are one entry.
static char *stack_from(Str **frames, int64_t *counts, int64_t runs) {
  size_t n = 16;
  for (int64_t i = 0; i < runs && i < FRAMES; i++) n += ((size_t)frames[i]->len + 4) * (counts[i] < 4 ? (size_t)counts[i] : 1) + 32;
  char *t = malloc(n), *p = t;
  *p = 0;
  for (int64_t i = 0; i < runs && i < FRAMES; i++) {
    if (counts[i] >= 4) p += sprintf(p, "%s%.*s (%lld times)", i ? " -> " : "", (int)frames[i]->len, frames[i]->bytes, (long long)counts[i]);
    else
      for (int64_t k = 0; k < counts[i]; k++) p += sprintf(p, "%s%.*s", i || k ? " -> " : "", (int)frames[i]->len, frames[i]->bytes);
  }
  if (runs > FRAMES) strcpy(p, " -> ...");
  return t;
}
#else
// (core.fault's, through core.rt)
char *strata_stack_text(Str **frames, int64_t *counts, int64_t runs);
static char *stack_from(Str **frames, int64_t *counts, int64_t runs) { return strata_stack_text(frames, counts, runs); }
#endif

static char *stack_text(void) { return stack_from(CALLS.frames, CALLS.counts, CALLS.runs); }

static void print_stack(void) {
  if (CALLS.runs == 0) return;
  char *t = stack_text();
  fprintf(stderr, "    stack: %s\n", t);
  free(t);
}

// ---- tests ----
//
// `strata test` with the JIT calls each test's function through
// `strata_test`: a fault ends the test (back here, not out of the process),
// and what it prints is kept, as the interpreter does.

// A run's state is per thread: the JIT may run code on several threads at
// once (compile-time code in one, a test or fuzz case in another).
static void catch_overflow(void);

#ifndef STRATA_CORE_RT
static void keep_printed(const char *b, size_t n) {
  if (RS->printed_len + n + 1 > RS->printed_cap) RS->printed = realloc(RS->printed, RS->printed_cap = (RS->printed_len + n + 1) * 2);
  memcpy(RS->printed + RS->printed_len, b, n);
  RS->printed_len += n;
  RS->printed[RS->printed_len] = 0;
}
#else
// (core.test's: what a test printed, kept)
void strata_keep_printed(const char *b, int64_t n);
#define keep_printed(b, n) strata_keep_printed((b), (int64_t)(n))
#endif

// Run test `f`: 0 when it got to its end, 1 after a fault (then
// `strata_test_fault` says which). Each starts as the interpreter's: no
// call stack, the generators' numbers from the start, nothing printed.
static void forget_senders(void);

// A run inside `strata_test` (a test, a fuzz case): a panic there fails it
// instead of ending the process. Its tasks share it: once one of them
// panics, every thread of the run is cancelled (it stops at its next wait),
// and the end of the task's scope raises the panic in the test's thread.
//
// A supervised task (`spawn supervised e`) is such a boundary too, for
// itself and the tasks below it: its first panic is recorded (kind, message,
// call stack), every thread inside stops, and the task's result is
// `Err(Panic { … })`. Inside one, a panic returns and the code leaves as a
// cancelled task does, dropping what it holds: one in compiled code
// (`strata_panic`), and one the runtime finds in a builtin (`soft_fault`: a
// division by zero, a negative shift, `sqrt` of a negative number, ...: the
// builtin gives back a harmless value, and the compiled code leaves right
// after the call). Only a stack overflow and running out of memory still
// jump out (`strata_fault`), leaving what the task's frames held allocated.
typedef struct Contained {
  int32_t panicked;
  _Bool supervised;
  int32_t kind;     // the first panic (a supervised boundary's)
  Str *msg;
  char *stack;
} Contained;
static _Thread_local Contained *contained;

// (see "knowing when every thread of a run waits", below)
struct Timeline;
struct Task;
typedef struct Waiter {
  int64_t gen;           // `wake_gen` when it began to wait; -1: not waiting
  const char *what;      // the wait it is in
  _Bool timed;           // its wait ends by itself (a real clock's deadline)
  struct Timeline *vt;   // a `VirtualClock`'s time it waits on, and the deadline
  int64_t vdeadline;
  Str **frames;          // its call stack (its thread's; NULL before it runs)
  int64_t *counts, *runs;
  struct Task *task;     // NULL: the run's first thread
  const char *origin;    // where its task was spawned, if known
  struct Waiter *next, **back;
} Waiter;

typedef struct Run {
  Waiter *first, **tail;  // in the order they started
  Contained *test;       // the test's boundary (NULL: the program)
  struct Timeline *vtime;  // its `VirtualClock`'s time (made at its first use)
  Waiter root;           // its first thread's
} Run;

static int64_t wake_gen;  // under `chan_lock`
static _Thread_local Run *my_run;
static _Thread_local Waiter *me;

static void add_waiter(Run *r, Waiter *w) {
  if (!r->tail) r->tail = &r->first;
  w->gen = -1;
  w->next = NULL;
  w->back = r->tail;
  *r->tail = w;
  r->tail = &w->next;
}

static void remove_waiter(Run *r, Waiter *w) {
  *w->back = w->next;
  if (w->next) w->next->back = w->back;
  else r->tail = w->back;
}

// Cancellations pending (tasks told to stop, runs that panicked): while it
// is 0, compiled code doesn't look further (`cancel_point` in llvm.rs).
int32_t strata_cancel_any;

// The part of a run the C runtime keeps while tasks are its own: the
// boundary its panics stop at (`Contained`) and its threads (`Run`), the
// caller's kept aside. core.test's `strata_test` calls these, as the first
// stage's below does.
typedef struct {
  Contained run, *outer;
  Run trun, *outer_run;
  Waiter *outer_me;
} RunHold;

void *strata_run_begin(void) {
  catch_overflow();
  forget_senders();
  RunHold *h = calloc(1, sizeof *h);
  h->outer = contained;
  contained = &h->run;
  // its own run: its threads' waits are counted apart from the program's
  h->outer_run = my_run;
  h->outer_me = me;
  h->trun.test = &h->run;
  h->trun.root.frames = CALLS.frames;
  h->trun.root.counts = CALLS.counts;
  h->trun.root.runs = &CALLS.runs;
  add_waiter(&h->trun, &h->trun.root);
  my_run = &h->trun;
  me = &h->trun.root;
  return h;
}

// The run is over (`failed`: a fault left it): 1 when it failed, a panic of
// the run itself (a deadlock) included, which is then the run's fault.
int64_t strata_run_end(void *hold, int64_t failed) {
  RunHold *h = hold;
  contained = h->outer;
  my_run = h->outer_run;
  me = h->outer_me;
  free(h->trun.vtime);
  int64_t r = failed;
  if (!failed && h->run.panicked && h->run.msg) {
    r = 1;
    RS->fault_kind = h->run.kind;
    RS->fault_msg = h->run.msg;
    free(RS->fault_stack);
    RS->fault_stack = h->run.stack;
  }
  if (h->run.panicked) __atomic_sub_fetch(&strata_cancel_any, 1, __ATOMIC_RELAXED);
  free(h);
  return r;
}

// A run a fault can leave (core.mem's `trap_run` / `trap_leave`): `f` runs
// with `buf` (a `sigjmp_buf`, `trap_size` bytes) set in this frame, which
// lives until `f` is done; `strata_trap_leave(buf)` comes back here: 1.
int64_t strata_trap_run(void *buf, void (*f)(void)) {
  if (sigsetjmp(*(sigjmp_buf *)buf, 1) == 0) {
    f();
    return 0;
  }
  return 1;
}

_Noreturn void strata_trap_leave(void *buf) { siglongjmp(*(sigjmp_buf *)buf, 1); }

int64_t strata_trap_size(void) { return (int64_t)sizeof(sigjmp_buf); }

#ifndef STRATA_CORE_RT
int32_t strata_test(void (*f)(void)) {
  sigjmp_buf here;
  // the caller's own call stack (a compiled host, such as the compiler
  // running compile-time code in its own process, has frames of its own),
  // kept aside while the run's starts from nothing, then put back
  int64_t outer_runs = CALLS.runs, kept = outer_runs < FRAMES ? outer_runs : FRAMES;
  Str **outer_frames = NULL;
  int64_t *outer_counts = NULL;
  if (kept > 0) {
    outer_frames = malloc((size_t)kept * sizeof(Str *));
    outer_counts = malloc((size_t)kept * sizeof(int64_t));
    memcpy(outer_frames, CALLS.frames, (size_t)kept * sizeof(Str *));
    memcpy(outer_counts, CALLS.counts, (size_t)kept * sizeof(int64_t));
  }
  CALLS.runs = 0;
  RS->rng = RS->test_seed;
  RS->printed_len = 0;
  if (RS->printed) RS->printed[0] = 0;
  RS->capturing = 1;
  void *hold = strata_run_begin();
  RS->trap = &here;
  _Bool failed = 1;
  if (sigsetjmp(here, 1) == 0) {
    f();
    failed = 0;
  }
  RS->trap = NULL;
  RS->capturing = 0;
  int32_t r = (int32_t)strata_run_end(hold, failed);
  if (kept > 0) {
    memcpy(CALLS.frames, outer_frames, (size_t)kept * sizeof(Str *));
    memcpy(CALLS.counts, outer_counts, (size_t)kept * sizeof(int64_t));
    free(outer_frames);
    free(outer_counts);
  }
  CALLS.runs = outer_runs;
  return r;
}
#endif

// ---- fuzzing (`strata verify --jit`) ----
//
// A fuzz driver (llvm.rs, `compile_fuzz`) generates a function's arguments,
// hands each to strata (`strata_fuzz_keep`, src/llvm/jit.rs: read by its
// shape), and calls it; `strata_fuzz` runs one case as
// `strata_test` runs a test, with the generators seeded and a step budget
// (each loop iteration and call is a step): past it, a `Limit` fault.

_Noreturn void strata_fault(int32_t kind, Str *msg);


#ifndef STRATA_CORE_RT
void strata_step(void) {
  if (RS->counting && --RS->steps < 0) {
    RS->counting = 0;
    strata_fault(6, str_c("too many steps"));
  }
}

// The running case's arguments as code, for the compiler written in Strata
// (compiler/codegen's drivers with `--fuzz-text`: `strata_fuzz_arg`), each
// followed by a 0x1f byte; emptied when a case starts.

void strata_fuzz_arg(Str *s) {
  RS->fuzz_args = realloc(RS->fuzz_args, RS->fuzz_args_len + (size_t)s->len + 2);
  memcpy(RS->fuzz_args + RS->fuzz_args_len, s->bytes, (size_t)s->len);
  RS->fuzz_args_len += (size_t)s->len;
  RS->fuzz_args[RS->fuzz_args_len++] = 0x1f;
  RS->fuzz_args[RS->fuzz_args_len] = 0;
}


int32_t strata_fuzz(void (*f)(void), int64_t seed, int64_t budget) {
  RS->fuzz_args_len = 0;
  if (RS->fuzz_args) RS->fuzz_args[0] = 0;
  RS->steps = budget;
  RS->counting = 1;
  RS->test_seed = (uint64_t)seed;
  int32_t r = strata_test(f);
  RS->test_seed = 7;
  RS->counting = 0;
  return r;
}

void strata_seed(int64_t seed) { RS->rng = (uint64_t)seed; }
#endif

// The `Code` capability (std.code): the codebase as typed values, which
// strata builds (src/ui.rs) and writes into this program's memory
// (src/llvm/native.rs) when it runs one. A built program has none.
typedef Str *(*CodeVersion)(void);
typedef void *(*CodeSnapshot)(Str **err);
static CodeVersion code_version;
static CodeSnapshot code_snapshot;
void strata_set_code_host(CodeVersion v, CodeSnapshot s) {
  code_version = v;
  code_snapshot = s;
}

static void no_code(void) { strata_fault(4, str_c("`Codebase` is given only under `strata run` and `strata serve` (a built program has none)")); }

Str *strata_host_code_version(void) {
  if (!code_version) no_code();
  return code_version();
}

// The snapshot, or NULL and the failure in `*err`.
void *strata_host_code_snapshot(Str **err) {
  if (!code_snapshot) no_code();
  return code_snapshot(err);
}

#ifndef STRATA_CORE_RT
// Values the host wrote into memory for compiled code to take
// (`strata_run_call`'s arguments; llvm.rs, `Entry::Call`).
void strata_set_given(int64_t *g) { RS->given = g; }
int64_t strata_given(int64_t i) { return RS->given[i]; }

// Compile-time code's value (`strata.ct.N` hands it over), for the compiler
// to read (src/llvm/native.rs).
void strata_ct_keep(int64_t v) { RS->ct_kept = v; }
int64_t strata_ct_kept(void) { return RS->ct_kept; }
#endif

// The running case's arguments (`strata_fuzz_arg`'s), for strata.
const char *strata_fuzz_args(void) { return RS->fuzz_args ? RS->fuzz_args : ""; }

// The last test's fault: its kind, message and call stack, and what it printed.
const char *strata_test_kind(void) { return KINDS[RS->fault_kind & 255]; }
Str *strata_test_message(void) { return RS->fault_msg; }
const char *strata_test_stack(void) { return RS->fault_stack ? RS->fault_stack : ""; }
const char *strata_test_printed(void) { return RS->printed ? RS->printed : ""; }

// A fault: its kind, then 256 if the interpreter shows no call stack for it.
static void finish_open_scopes(void);
struct Contained;
static void panic_contained(struct Contained *c);
static void record_panic(struct Contained *c, int32_t kind, Str *msg, char *stack);
static char *panic_stack(void);

// A fault: of this thread (`task_stack` NULL), or of a task it started
// (raised here by `await` or the end of its scope), whose call stack then
// follows this thread's. Caught by a trap (a test, a task), every task this
// thread started finishes first.
static _Thread_local char *task_origin;  // where this thread's task was spawned

// What core.fault's `strata_fault_at` asks of the C runtime while tasks and
// output are its own (core.mem's builtins): where this thread's task was
// spawned, a caught fault's effect on the run's tasks, stdout flushed (and
// held: no other thread prints after a fault ends the program), a write to a
// file descriptor, the end of the process, a thread that only waits.
char *strata_task_origin(void) { return task_origin; }

void strata_contain_fault(int64_t kind, Str *msg) {
  // (a test's tasks stop, rather than wait for what won't come; a
  // supervised task's panic is kept for its result)
  if (contained && contained->supervised) record_panic(contained, (int32_t)kind & 255, msg, (kind & 256) ? strdup("") : panic_stack());
  else if (contained) panic_contained(contained);
  finish_open_scopes();
}

void strata_out_flush(void) {
  flockfile(stdout);
  fflush(stdout);
}

void strata_fd_write(int64_t fd, const char *p, int64_t n) {
  while (n > 0) {
    ssize_t w = write((int)fd, p, (size_t)n);
    if (w <= 0) break;
    p += w;
    n -= w;
  }
}

_Noreturn void strata_process_exit(int64_t code) { exit((int)code); }

_Noreturn void strata_thread_park(void) {
  for (;;) pause();
}

#ifndef STRATA_CORE_RT
static _Noreturn void fault_at(int32_t kind, Str *msg, char *task_stack) {
  char *stack = NULL;
  if (!(kind & 256)) {
    char *here = stack_text();
    if (!task_stack && task_origin && !RS->trap) {
      // a panic in a task, ending the program: where it was spawned, then where it is
      stack = malloc(strlen(task_origin) + strlen(here) + 16);
      sprintf(stack, "%s%sspawn%s%s", task_origin, *task_origin ? " -> " : "", *here ? " -> " : "", here);
      free(here);
    } else if (task_stack) {
      stack = malloc(strlen(here) + strlen(task_stack) + 16);
      sprintf(stack, "%s%sspawn%s%s", here, *here ? " -> " : "", *task_stack ? " -> " : "", task_stack);
      free(here);
    } else {
      stack = here;
    }
  }
  free(task_stack);
  if (RS->trap) {
    // (a test's tasks stop, rather than wait for what won't come; a
    // supervised task's panic is kept for its result)
    if (contained && contained->supervised) record_panic(contained, kind & 255, msg, (kind & 256) ? strdup("") : panic_stack());
    else if (contained) panic_contained(contained);
    finish_open_scopes();
    RS->fault_kind = kind;
    RS->fault_msg = msg;
    free(RS->fault_stack);
    RS->fault_stack = stack;
    siglongjmp(*RS->trap, 1);
  }
  // a panic ends the program, whichever thread it is in (the first to
  // panic reports; another waits for the end)
  static int32_t ending;
  if (__atomic_exchange_n(&ending, 1, __ATOMIC_ACQ_REL)) for (;;) pause();
  flockfile(stdout);  // (no other thread prints after this)
  fflush(stdout);
  fprintf(stderr, "fault (%s): %.*s\n", KINDS[kind & 255], (int)msg->len, msg->bytes);
  if (stack && *stack) fprintf(stderr, "    stack: %s\n", stack);
  exit(101);
}

_Noreturn void strata_fault(int32_t kind, Str *msg) { fault_at(kind, msg, NULL); }
#else
// (core.fault's)
_Noreturn void strata_fault_at(int32_t kind, Str *msg, char *task_stack);
#define fault_at(k, m, t) strata_fault_at((k), (m), (t))
#endif


// ---- tasks (`scope`, `spawn`, `await`) ----
//
// A task runs on a thread of its own. Its closure (holding what it
// captured) and its result belong to its scope: the end of the scope waits
// for every task, then drops them. A panic in a task ends the program, as
// anywhere; inside a test it fails the test (see `Contained`).
//
// Cancelling a task (a `race`'s losers) stops it at its next wait: `send`,
// `recv`, `select` and `await` give up, and the compiled code leaves each
// function as `return` would, dropping what it holds (`cancel_point` in
// llvm.rs), so nothing runs after the wait and nothing leaks. A task is
// cancelled when it or a task above it (the one that spawned it, ...) is.

#define TASK_STACK ((size_t)64 << 20)

// Channels, and tasks' ends, under one lock: what changes wakes every
// waiter, which looks again (simple; a lock per channel can come later).
static pthread_mutex_t chan_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t chan_cond = PTHREAD_COND_INITIALIZER;

typedef struct Channel {
  int64_t cap;               // room for values waiting (0: a send waits for its receiver)
  int64_t *items;            // a ring of slots
  int64_t head, len, size;
  int64_t senders;           // copies of the sender not closed yet: one per task holding it
  int64_t sent, taken;       // values sent and received so far
  int64_t waiting;           // receivers waiting (an unbuffered send in a `select` can go)
  void (*drop)(int64_t);     // drops a value left in it (NULL: nothing to drop)
} Channel;

// The senders this thread holds a copy of and hasn't closed: the channels
// it made, or a task's shares (`strata_spawn`).
static _Thread_local Channel **held;
static _Thread_local int64_t nheld, capheld;

static void hold_sender(Channel *c) {
  if (nheld == capheld) held = realloc(held, (size_t)(capheld = capheld ? capheld * 2 : 8) * sizeof *held);
  held[nheld++] = c;
}

static _Bool holds_sender(Channel *c) {
  for (int64_t i = 0; i < nheld; i++)
    if (held[i] == c) return 1;
  return 0;
}

// A test starts holding none.
static void forget_senders(void) { nheld = 0; }

// Whether this thread held `c`, which it no longer does.
static _Bool unhold_sender(Channel *c) {
  for (int64_t i = 0; i < nheld; i++)
    if (held[i] == c) {
      held[i] = held[--nheld];
      return 1;
    }
  return 0;
}

// ---- knowing when every thread of a run waits ----
//
// A run is the program, or one `strata test` or fuzz case: its first thread
// and every task started under it. Each has a `Waiter` in its run's list,
// from its `spawn` to its end. A thread about to wait in a Strata wait (a
// `recv`, `send`, `select`, `await`, the end of a `scope`, a sleep) says so
// under `chan_lock` (`wait_begin`), and stops saying so when it wakes
// (`wait_end`). Every wake-up of `chan_cond` (`wake_all`) moves `wake_gen`
// on, so a thread woken but not yet running again doesn't count as waiting.
// When the last thread of a run starts to wait and no wait ends by itself (a
// real clock's deadline), nothing can happen any more: a virtual clock's time
// jumps to its next deadline (`stuck`), or, if none waits on one, that is a
// deadlock, a panic. Host I/O (the console, files) isn't a wait here: a
// thread in it can still go on by itself.
// Wake every waiter (holding `chan_lock`): something they wait on may have changed.
static void wake_all(void) {
  wake_gen++;
  pthread_cond_broadcast(&chan_cond);
}

// This thread's run: the program's is made at its first wait or `spawn`.
static Run *this_run(void) {
  if (!my_run) {
    Run *r = calloc(1, sizeof *r);
    r->root.frames = CALLS.frames;
    r->root.counts = CALLS.counts;
    r->root.runs = &CALLS.runs;
    add_waiter(r, &r->root);
    my_run = r;
    me = &r->root;
  }
  return my_run;
}

static _Bool stuck(void);

// About to wait (holding `chan_lock`), on what is `what` names and, for a
// `VirtualClock`, its time and deadline: 0 when it shouldn't (it was the last
// thread to wait, and a virtual clock's time moved, or the run deadlocked
// in a test: it looks again).
static _Bool wait_begin_v(const char *what, struct Timeline *vt, int64_t vdeadline) {
  this_run();
  me->gen = wake_gen;
  me->what = what;
  me->timed = 0;
  me->vt = vt;
  me->vdeadline = vdeadline;
  return !stuck();
}

static _Bool wait_begin(const char *what) { return wait_begin_v(what, NULL, 0); }

// About to wait for a real clock's deadline (or a short while): it ends by itself.
static void timed_begin(const char *what) {
  this_run();
  me->gen = wake_gen;
  me->what = what;
  me->timed = 1;
  me->vt = NULL;
}

static void wait_end(void) {
  if (me) me->gen = -1;
}

// One wait on `chan_cond` (holding `chan_lock`), counted.
#define WAIT(what) \
  do { \
    if (wait_begin(what)) pthread_cond_wait(&chan_cond, &chan_lock); \
    wait_end(); \
  } while (0)

typedef struct Task {
  pthread_t thread;
  void *clo;                  // the closure: its function, its drop, ...
  void (*run)(void *, void *);  // calls the closure, stores the result
  void (*drop)(void *);       // drops a stored result (NULL: nothing to drop)
  int64_t out;                // the result (every compiled value fits)
  _Bool joined, faulted, capturing, counting;
  int32_t kind;
  Str *msg;
  char *stack, *printed;
  size_t printed_len;
  uint64_t seed;
  int64_t steps, *given;
  Channel **shares;           // the senders it holds a copy of
  int64_t nshares;
  _Bool done;                 // finished (set under `chan_lock`, for `select`)
  int32_t cancel;             // told to stop (counted in `strata_cancel_any`)
  _Bool unwound;              // stopped by a cancellation: no result
  struct Task *parent;        // the task that spawned it (NULL: the main thread)
  Contained *contained;       // the boundary it runs in (a test, a supervised task), if any
  void (*fail)(void *, Str *, Str *, Str *);  // a supervised task's: stores `Err(Panic { … })`
  Contained boundary;         // a supervised task's own
  char *origin;               // the call stack where it was spawned
  Run *run_of;                // the run it belongs to
  Waiter w;                   // its place in that run's list
  struct Task *next;
} Task;

typedef struct TaskScope {
  Task *first, *last;
  struct TaskScope *up;
} TaskScope;

static _Thread_local TaskScope *open_scopes;
static _Thread_local Task *self_task;  // the task this thread runs (NULL: the main thread)
static _Thread_local _Bool unwound;    // the last function left by a cancellation
static void free_alt_stack(void);

// Whether this thread should stop at its next wait.
_Bool strata_cancelled(void) {
  if (contained && __atomic_load_n(&contained->panicked, __ATOMIC_RELAXED)) return 1;
  for (Task *t = self_task; t; t = t->parent)
    if (__atomic_load_n(&t->cancel, __ATOMIC_RELAXED)) return 1;
  return 0;
}

// The compiled code left a function because of a cancellation.
void strata_unwound(void) { unwound = 1; }

// ---- waiting for input, cancellably ----
//
// A host read (stdin's `read_line`; a socket's, with std.net) can wait for
// ever, outside `chan_lock`'s condition variable. A thread that can be
// cancelled (a task's, or a test's) waits for its descriptor with `poll`,
// together with a wake pipe of its own; a cancellation writes a byte to the
// pipe of every thread waiting so (`wake_readers`), which looks again: if it
// was cancelled the read gives up and the compiled code leaves
// (`cancel_point`), as after any other wait. Only threads in such a wait are
// woken, and code that doesn't wait for input pays nothing.

typedef struct Reader {
  int wake[2];  // the pipe: read end, write end (both non-blocking)
  struct Reader *next, *prev;
} Reader;
static Reader *readers;                // threads waiting for input (under `chan_lock`)
static _Thread_local Reader *my_reader;  // this thread's pipe, made at its first wait

// Wake every thread waiting for input (under `chan_lock`).
static void wake_readers(void) {
  for (Reader *r = readers; r; r = r->next) {
    char b = 1;
    if (write(r->wake[1], &b, 1) < 0) { /* full: a wake is already pending */ }
  }
}

static void unlink_reader(Reader *r) {
  if (r->prev) r->prev->next = r->next;
  else readers = r->next;
  if (r->next) r->next->prev = r->prev;
  r->next = r->prev = NULL;
}

// Wait until `fd` can be read from (or is at its end, or failed: the read
// says which): 1; or until this thread is cancelled: 0. A thread nothing can
// cancel (the main thread outside a test) doesn't wait here: 1 at once.
static _Bool wait_readable(int fd) {
  if (!self_task && !contained) return 1;
  if (!my_reader) {
    Reader *r = calloc(1, sizeof *r);
    if (!r || pipe(r->wake) != 0) {
      free(r);
      return 1;  // no pipe: an uncancellable wait, as before
    }
    for (int i = 0; i < 2; i++) {
      fcntl(r->wake[i], F_SETFL, fcntl(r->wake[i], F_GETFL) | O_NONBLOCK);
      fcntl(r->wake[i], F_SETFD, FD_CLOEXEC);
    }
    my_reader = r;
  }
  for (;;) {
    pthread_mutex_lock(&chan_lock);
    if (strata_cancelled()) {
      pthread_mutex_unlock(&chan_lock);
      return 0;
    }
    my_reader->next = readers;
    if (readers) readers->prev = my_reader;
    readers = my_reader;
    pthread_mutex_unlock(&chan_lock);
    struct pollfd p[2] = {{.fd = fd, .events = POLLIN}, {.fd = my_reader->wake[0], .events = POLLIN}};
    int n = poll(p, 2, -1);
    pthread_mutex_lock(&chan_lock);
    unlink_reader(my_reader);
    pthread_mutex_unlock(&chan_lock);
    char drain[64];
    while (read(my_reader->wake[0], drain, sizeof drain) > 0) {}
    if (n < 0 && errno != EINTR) return 1;  // let the read report it
    if (n > 0 && p[0].revents) return 1;
    // woken (by a cancellation, maybe another thread's) or interrupted: look again
  }
}

// A thread's pipe, closed when its task ends.
static void forget_reader(void) {
  if (!my_reader) return;
  close(my_reader->wake[0]);
  close(my_reader->wake[1]);
  free(my_reader);
  my_reader = NULL;
}

// Tell a task to stop (under `chan_lock`; waiters are woken by the caller,
// except those waiting for input, woken here).
static void cancel_task(Task *t) {
  if (t->done || t->cancel) return;
  __atomic_store_n(&t->cancel, 1, __ATOMIC_RELAXED);
  __atomic_add_fetch(&strata_cancel_any, 1, __ATOMIC_RELAXED);
  wake_readers();
}

// A race's losers: every task of the scope not finished yet.
void strata_scope_cancel(TaskScope *s) {
  pthread_mutex_lock(&chan_lock);
  for (Task *t = s->first; t; t = t->next) cancel_task(t);
  wake_all();
  pthread_mutex_unlock(&chan_lock);
}

// A panic inside a boundary (a test, a supervised task): every thread
// inside stops; the first panic's kind, message and stack are kept (the
// strings are the boundary's; `msg` NULL: not known here).
static void record_panic(Contained *c, int32_t kind, Str *msg, char *stack) {
  pthread_mutex_lock(&chan_lock);
  if (!c->panicked) {
    c->panicked = 1;
    c->kind = kind;
    c->msg = msg;
    c->stack = stack;
    __atomic_add_fetch(&strata_cancel_any, 1, __ATOMIC_RELAXED);
    wake_readers();
  } else {
    free(stack);
  }
  wake_all();
  pthread_mutex_unlock(&chan_lock);
}

static void panic_contained(Contained *c) { record_panic(c, 4, NULL, NULL); }

// Where a panic happened, from `main` through each `spawn`.
static char *panic_stack(void) {
  char *here = stack_text();
  if (!task_origin) return here;
  char *s = malloc(strlen(task_origin) + strlen(here) + 16);
  sprintf(s, "%s%sspawn%s%s", task_origin, *task_origin ? " -> " : "", *here ? " -> " : "", here);
  free(here);
  return s;
}

// A panic in compiled code: inside a supervised task it is the task's
// (recorded; the code then leaves its functions, see `fault` in llvm.rs);
// anywhere else it ends the program, or the test, as `strata_fault`.
void strata_panic(int32_t kind, Str *msg) {
  if (contained && contained->supervised) {
    record_panic(contained, kind & 255, msg, (kind & 256) ? strdup("") : panic_stack());
    return;
  }
  fault_at(kind, msg, NULL);
}

// A panic a builtin finds (a division by zero, `ord` of a long string, ...):
// inside a supervised task it is the task's, recorded, and this returns 1:
// the builtin then gives back a harmless value (0, an empty string), which
// the compiled code never uses, since it checks for the panic right after
// the call (`soft_faults` in llvm.rs) and leaves. Anywhere else it ends the
// program, or the test, as `strata_fault`.
static _Bool soft_fault(int32_t kind, Str *msg) {
  if (contained && contained->supervised) {
    record_panic(contained, kind & 255, msg, (kind & 256) ? strdup("") : panic_stack());
    return 1;
  }
  fault_at(kind, msg, NULL);
}

// `soft_fault` for core.rt (core.mem's `raw_fault`), which takes `msg`.
_Bool strata_soft_fault(int64_t kind, Str *msg) { return soft_fault((int32_t)kind, msg); }

// The empty string a builtin gives back after a contained panic: a
// constant (a count of 0), never freed.
static int64_t no_str_words[2] = {0, 0};
#define NO_STR ((Str *)&no_str_words[1])

// Whether the function just called was left by a cancellation or a
// supervised task's panic (it gave no result).
_Bool strata_was_unwound(void) { return unwound; }

static void *task_main(void *arg) {
  Task *t = arg;
  catch_overflow();
  RS->rng = t->seed;
  RS->capturing = t->capturing;
  RS->counting = t->counting;
  RS->steps = t->steps;
  RS->given = t->given;
  for (int64_t i = 0; i < t->nshares; i++) hold_sender(t->shares[i]);
  self_task = t;
  contained = t->contained;
  task_origin = t->origin;
  unwound = 0;
  pthread_mutex_lock(&chan_lock);
  my_run = t->run_of;
  me = &t->w;
  t->w.frames = CALLS.frames;
  t->w.counts = CALLS.counts;
  t->w.runs = &CALLS.runs;
  pthread_mutex_unlock(&chan_lock);
  // outside a test a panic ends the program (no trap); inside one it ends the task
  sigjmp_buf here;
  RS->trap = contained ? &here : NULL;
  if (!contained || sigsetjmp(here, 1) == 0) {
    t->run(t->clo, &t->out);
    t->unwound = unwound;
  } else if (!t->fail) {
    t->faulted = 1;
    t->kind = RS->fault_kind;
    t->msg = RS->fault_msg;
    t->stack = RS->fault_stack;
    RS->fault_stack = NULL;
    panic_contained(contained);
  }
  RS->trap = NULL;
  // a supervised task that panicked: its result is the panic
  _Bool died = t->fail && t->boundary.panicked;
  t->printed = RS->printed;
  t->printed_len = RS->printed_len;
  RS->printed = NULL;
  RS->printed_len = RS->printed_cap = 0;
  // a copy of a sender it didn't close is closed now (its receivers would
  // wait forever), and that is a panic, unless the task was cancelled
  pthread_mutex_lock(&chan_lock);
  _Bool unclosed = nheld > 0 && !t->unwound && !died;
  for (int64_t i = 0; i < nheld; i++) held[i]->senders--;
  t->done = 1;
  remove_waiter(t->run_of, &t->w);
  me = NULL;
  my_run = NULL;
  wake_all();
  pthread_mutex_unlock(&chan_lock);
  free(held);
  held = NULL;
  nheld = capheld = 0;
  if (died) {
    Contained *b = &t->boundary;
    Str *msg = b->msg ? b->msg : str_c("a task below it panicked");
    t->fail(&t->out, str_c(KINDS[b->kind & 255]), msg, str_c(b->stack ? b->stack : ""));
    free(b->stack);
    t->unwound = 0;
    __atomic_sub_fetch(&strata_cancel_any, 1, __ATOMIC_RELAXED);
  } else if (unclosed && !t->faulted) {
    Str *m = str_c("a task ended without closing its copy of a sender (`tx.close()`)");
    if (!contained) fault_at(4, m, NULL);
    t->faulted = 1;
    t->kind = 4;
    t->msg = m;
    t->stack = strdup("");
    panic_contained(contained);
  }
  self_task = NULL;
  contained = NULL;
  task_origin = NULL;
  forget_reader();
  free_alt_stack();
#ifdef STRATA_CORE_RT
  // this thread's free lists and tallies back to the shared ones
  strata_memory_thread_end();
#endif
  return NULL;
}

void *strata_scope_begin(void) {
  TaskScope *s = calloc(1, sizeof *s);
  if (!s) strata_fault(6, str_c("out of memory"));
  s->up = open_scopes;
  open_scopes = s;
  return s;
}

// A task of the innermost scope open on this thread: the checker allows a
// `spawn` only inside a `scope` (a lambda's included, as a comprehension's
// body, which runs there).
void *strata_spawn(void *clo, void (*run)(void *, void *), void (*drop)(void *), void (*fail)(void *, Str *, Str *, Str *), Channel **shares, int64_t nshares) {
  TaskScope *s = open_scopes;
  if (!s) strata_fault(4, str_c("`spawn` ran outside its `scope` (a closure that spawns was called after the scope ended)"));
  Task *t = calloc(1, sizeof *t);
  if (!t) strata_fault(6, str_c("out of memory"));
  t->clo = clo;
  t->run = run;
  t->drop = drop;
  // the task's numbers: from this thread's, so a run repeats
  RS->rng = RS->rng * 6364136223846793005ULL + 1442695040888963407ULL;
  t->seed = RS->rng ^ (RS->rng >> 29);
  t->capturing = RS->capturing;
  t->counting = RS->counting;
  t->steps = RS->steps;
  t->given = RS->given;
  t->parent = self_task;
  t->contained = contained;
  if (fail) {
    t->fail = fail;
    t->boundary.supervised = 1;
    t->contained = &t->boundary;
  }
  {
    // for a panic's call stack: this task's origin, then where it is
    char *here = stack_text();
    if (task_origin) {
      t->origin = malloc(strlen(task_origin) + strlen(here) + 16);
      sprintf(t->origin, "%s%sspawn%s%s", task_origin, *task_origin ? " -> " : "", *here ? " -> " : "", here);
      free(here);
    } else {
      t->origin = here;
    }
  }
  // a copy of each sender it captured that this thread still holds: the
  // task's, to close (one this thread closed stays closed for it too)
  if (nshares) {
    t->shares = malloc((size_t)nshares * sizeof *shares);
    pthread_mutex_lock(&chan_lock);
    for (int64_t i = 0; i < nshares; i++)
      if (holds_sender(shares[i])) {
        shares[i]->senders++;
        t->shares[t->nshares++] = shares[i];
      }
    pthread_mutex_unlock(&chan_lock);
  }
  if (__atomic_fetch_add(&live_tasks, 1, __ATOMIC_RELAXED) == 0) set_threaded(1);
#ifdef STRATA_CORE_RT
  // this thread's open arenas are kept: the task may hold their objects
  strata_memory_spawning();
#endif
  // counted in its run from now: a task not started yet isn't waiting
  pthread_mutex_lock(&chan_lock);
  t->run_of = this_run();
  t->w.task = t;
  t->w.origin = t->origin;
  add_waiter(t->run_of, &t->w);
  pthread_mutex_unlock(&chan_lock);
  pthread_attr_t a;
  pthread_attr_init(&a);
  pthread_attr_setstacksize(&a, TASK_STACK);
  int failed = pthread_create(&t->thread, &a, task_main, t);
  pthread_attr_destroy(&a);
  if (failed) {
    if (__atomic_sub_fetch(&live_tasks, 1, __ATOMIC_RELAXED) == 0) set_threaded(0);
    t->joined = 1;
    t->faulted = 1;
    t->kind = 6;
    t->msg = str_c("no thread for a task (too many at once)");
    pthread_mutex_lock(&chan_lock);
    t->done = 1;
    remove_waiter(t->run_of, &t->w);
    wake_all();
    pthread_mutex_unlock(&chan_lock);
    for (int64_t i = 0; i < t->nshares; i++) {
      pthread_mutex_lock(&chan_lock);
      t->shares[i]->senders--;
      wake_all();
      pthread_mutex_unlock(&chan_lock);
    }
  }
  if (s->last) s->last->next = t;
  else s->first = t;
  s->last = t;
  return t;
}

static void join(Task *t) {
  if (t->joined) return;
  pthread_join(t->thread, NULL);
  t->joined = 1;
  if (__atomic_sub_fetch(&live_tasks, 1, __ATOMIC_RELAXED) == 0) set_threaded(0);
  if (t->printed) {
    if (RS->capturing) keep_printed(t->printed, t->printed_len);
    free(t->printed);
    t->printed = NULL;
  }
}

// Drop a finished scope's tasks: each result, closure and record.
static void release_scope(TaskScope *s) {
  for (Task *t = s->first, *next; t; t = next) {
    next = t->next;
    if (!t->faulted && !t->unwound && t->drop) t->drop(&t->out);
    if (t->cancel) __atomic_sub_fetch(&strata_cancel_any, 1, __ATOMIC_RELAXED);
    free(t->origin);
    if (strata_release(t->clo)) ((void (*)(void *))(uintptr_t)((int64_t *)t->clo)[1])(t->clo);
    free(t->stack);
    free(t->shares);
    free(t);
  }
  free(s);
}

void strata_scope_end(TaskScope *s) {
  // (a wait like the others: its tasks may all be waiting too)
  if (s->first) {
    pthread_mutex_lock(&chan_lock);
    for (;;) {
      Task *t = s->first;
      while (t && t->done) t = t->next;
      if (!t) break;
      WAIT("the end of a `scope`");
    }
    pthread_mutex_unlock(&chan_lock);
  }
  for (Task *t = s->first; t; t = t->next) join(t);
  open_scopes = s->up;
  for (Task *t = s->first; t; t = t->next) {
    // (inside a supervised task the panic is its own: the code leaves, cancelled)
    if (t->faulted && !(contained && contained->supervised)) {
      Str *msg = t->msg;
      int32_t kind = t->kind;
      char *st = t->stack;
      t->stack = NULL;
      release_scope(s);
      fault_at(kind, msg, st ? st : strdup(""));
    }
  }
  release_scope(s);
}

// The task's result, once it has finished; NULL if this task was cancelled
// while it waited (the compiled code then leaves: `cancel_point`).
void *strata_await(Task *t) {
  pthread_mutex_lock(&chan_lock);
  while (!t->done && !strata_cancelled()) WAIT("`await`");
  _Bool given_up = !t->done;
  pthread_mutex_unlock(&chan_lock);
  if (given_up) return NULL;
  join(t);
  // inside a supervised task, a panic below it is its own: it leaves (cancelled)
  if (t->faulted && contained && contained->supervised) return NULL;
  if (t->faulted) {
    char *st = t->stack;
    t->stack = NULL;
    fault_at(t->kind, t->msg, st ? st : strdup(""));
  }
  return &t->out;
}

// ---- channels (`channel()`, `send`, `recv`, `close`, `select`) ----
//
// A channel holds what was sent (moved in as slots) until it is received
// (moved out). It has ended once every copy of its sender is closed: then
// `recv` gives the values left, then nothing.

static void push_item(Channel *c, int64_t v) {
  if (c->len == c->size) {
    int64_t size = c->size ? c->size * 2 : 4, *items = malloc((size_t)size * sizeof *items);
    for (int64_t i = 0; i < c->len; i++) items[i] = c->items[(c->head + i) % c->size];
    free(c->items);
    c->items = items;
    c->head = 0;
    c->size = size;
  }
  c->items[(c->head + c->len++) % c->size] = v;
  c->sent++;
}

static int64_t pop_item(Channel *c) {
  int64_t v = c->items[c->head];
  c->head = (c->head + 1) % c->size;
  c->len--;
  c->taken++;
  return v;
}

// A new channel, held twice (its sender and its receiver), and a copy of
// its sender for this thread.
Channel *strata_channel_new(int64_t cap, void (*drop)(int64_t)) {
  if (cap < 0 && soft_fault(4, str_c("a channel's buffer can't be negative"))) return NULL;
  Channel *c = strata_alloc(sizeof(Channel));
  memset(c, 0, sizeof *c);
  *count_of(c) += 1;  // (keeping what else the count word holds: an arena's tag, an epoch)
  c->cap = cap;
  c->drop = drop;
  c->senders = 1;
  hold_sender(c);
  return c;
}

// One end fewer: the last drops what is still in it.
void strata_channel_drop(Channel *c) {
  if (!strata_release(c)) return;
  if (c->drop)
    for (int64_t i = 0; i < c->len; i++) c->drop(c->items[(c->head + i) % c->size]);
  free(c->items);
  strata_free(c);
}

void strata_channel_send(Channel *c, int64_t v) {
  pthread_mutex_lock(&chan_lock);
  if (c->senders == 0) {
    pthread_mutex_unlock(&chan_lock);
    if (c->drop) c->drop(v);
    soft_fault(4, str_c("`send` on a channel that has ended (every copy of its sender was closed)"));
    return;
  }
  while (c->cap > 0 && c->len >= c->cap && !strata_cancelled()) WAIT("`send` (the channel is full)");
  if (c->cap > 0 && c->len >= c->cap) {
    // cancelled while it waited: nothing sent
    pthread_mutex_unlock(&chan_lock);
    if (c->drop) c->drop(v);
    return;
  }
  push_item(c, v);
  int64_t mine = c->sent;
  wake_all();
  // unbuffered: until a receiver took it
  while (c->cap == 0 && c->taken < mine && !strata_cancelled()) WAIT("`send` (no one has taken it)");
  pthread_mutex_unlock(&chan_lock);
}

// The next value (into `out`), or 0 once the channel has ended and is empty.
_Bool strata_channel_recv(Channel *c, int64_t *out) {
  pthread_mutex_lock(&chan_lock);
  c->waiting++;
  wake_all();
  _Bool stop = 0;
  while (c->len == 0 && c->senders > 0 && !(stop = strata_cancelled())) WAIT("`recv`");
  c->waiting--;
  _Bool got = c->len > 0 && !stop;
  if (got) *out = pop_item(c);
  wake_all();
  pthread_mutex_unlock(&chan_lock);
  return got;
}

void strata_channel_close(Channel *c) {
  if (!unhold_sender(c)) {
    soft_fault(4, str_c("`close` of a sender this task doesn't hold, or has closed already"));
    return;
  }
  pthread_mutex_lock(&chan_lock);
  c->senders--;
  wake_all();
  pthread_mutex_unlock(&chan_lock);
}

// ---- the clock (std.time) ----
//
// Monotonic time in nanoseconds. Waiting for it is a wait like a `recv`:
// under `chan_lock`, on `chan_cond` with a deadline, so a cancellation wakes
// it and it gives up at once.

static int64_t mono_ns(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return (int64_t)t.tv_sec * 1000000000 + t.tv_nsec;
}

// Wait on `chan_cond` (holding `chan_lock`) until woken or `deadline`
// (monotonic) has come. `pthread_cond_timedwait` takes the wall clock.
static void wait_until(int64_t deadline) {
  int64_t left = deadline - mono_ns();
  if (left <= 0) return;
  struct timespec now, at;
  clock_gettime(CLOCK_REALTIME, &now);
  int64_t abs = (int64_t)now.tv_sec * 1000000000 + now.tv_nsec + left;
  at.tv_sec = abs / 1000000000;
  at.tv_nsec = abs % 1000000000;
  pthread_cond_timedwait(&chan_cond, &chan_lock, &at);
}

int64_t strata_host_now(void) { return mono_ns(); }

// The wall clock: nanoseconds since 1970-01-01 UTC.
int64_t strata_host_wall_now(void) {
  struct timespec t;
  clock_gettime(CLOCK_REALTIME, &t);
  return (int64_t)t.tv_sec * 1000000000 + t.tv_nsec;
}

void strata_host_sleep(int64_t ns) {
  int64_t deadline = mono_ns() + ns;
  pthread_mutex_lock(&chan_lock);
  while (mono_ns() < deadline && !strata_cancelled()) {
    timed_begin("`sleep`");
    wait_until(deadline);
    wait_end();
  }
  pthread_mutex_unlock(&chan_lock);
}

// ---- virtual time (std.time.VirtualClock) ----
//
// A run's own time (`Timeline`), at 0 when it starts (each test has its
// own), shared by its tasks: it moves by itself, to the nearest deadline
// waited on, when every thread of the run waits (`stuck`), which wakes the
// sleepers and the `select`s with one of its timers. The capability holds
// nothing: its methods find the run's time here. Read and moved under
// `chan_lock`; not counted (it lives as long as its run).

typedef struct Timeline {
  int64_t now;  // nanoseconds from the run's start
} Timeline;

Timeline *strata_virtual_timeline(void) {
  pthread_mutex_lock(&chan_lock);
  Run *r = this_run();
  if (!r->vtime) r->vtime = calloc(1, sizeof(Timeline));
  Timeline *t = r->vtime;
  pthread_mutex_unlock(&chan_lock);
  return t;
}

int64_t strata_virtual_now(void) {
  Timeline *t = strata_virtual_timeline();
  pthread_mutex_lock(&chan_lock);
  int64_t v = t->now;
  pthread_mutex_unlock(&chan_lock);
  return v;
}

// Wait until the run's time has moved `ns` past now; a cancelled task stops at once.
void strata_virtual_sleep(int64_t ns) {
  Timeline *t = strata_virtual_timeline();
  pthread_mutex_lock(&chan_lock);
  int64_t deadline = t->now + ns;
  while (t->now < deadline && !strata_cancelled()) {
    if (wait_begin_v("a `VirtualClock`'s `sleep`", t, deadline)) pthread_cond_wait(&chan_cond, &chan_lock);
    wait_end();
  }
  pthread_mutex_unlock(&chan_lock);
}

// What a `select` arm waits on: kind 0 `recv` (`ok` says whether a value
// came, into `slot`), 1 `send` (of `slot`), 2 `await` (a task), 3 a timer of
// the system clock (`clock.after(d)`: `slot` is its deadline, monotonic
// nanoseconds), 4 a timer of another clock (`slot` is 1 once that clock said
// its deadline has passed: the compiled code asks the clock, `strata_select`
// gives -2 when it should ask again), 5 a timer of a `VirtualClock` (`target`
// the run's `Timeline`, `slot` its deadline in that time: moving the time wakes it).
typedef struct {
  int64_t kind;
  void *target;
  int64_t slot, ok;
} SelectOp;

// Wait until one of `ops` can happen, do it, and say which. Arms are looked
// at from a different one each time (from this thread's numbers), so none
// is starved.
int64_t strata_select(SelectOp *ops, int64_t n) {
  pthread_mutex_lock(&chan_lock);
  if (strata_cancelled()) goto give_up;
  for (int64_t i = 0; i < n; i++)
    if (ops[i].kind == 1 && ((Channel *)ops[i].target)->senders == 0) {
      pthread_mutex_unlock(&chan_lock);
      soft_fault(4, str_c("`send` on a channel that has ended (every copy of its sender was closed)"));
      // (contained: nothing happened, as for a cancelled `select`)
      for (int64_t k = 0; k < n; k++) {
        Channel *c = ops[k].target;
        if (ops[k].kind == 1 && c->drop) c->drop(ops[k].slot);
      }
      return -1;
    }
  RS->rng = RS->rng * 6364136223846793005ULL + 1442695040888963407ULL;
  int64_t start = (int64_t)((RS->rng >> 33) % (uint64_t)n);
  // the first of the system clock's deadlines; whether another clock's
  // timer needs asking again (then this wait is short)
  int64_t first = INT64_MAX;
  _Bool other_clocks = 0;
  for (int64_t i = 0; i < n; i++) {
    if (ops[i].kind == 3 && ops[i].slot < first) first = ops[i].slot;
    if (ops[i].kind == 4) other_clocks = 1;
  }
  for (int64_t i = 0; i < n; i++)
    if (ops[i].kind == 0) ((Channel *)ops[i].target)->waiting++;
  wake_all();
  int64_t chosen = -1;
  while (chosen < 0) {
    for (int64_t k = 0; k < n && chosen < 0; k++) {
      int64_t i = (start + k) % n;
      SelectOp *op = &ops[i];
      if (op->kind == 0) {
        Channel *c = op->target;
        if (c->len > 0) {
          op->slot = pop_item(c);
          op->ok = 1;
          chosen = i;
        } else if (c->senders == 0) {
          op->ok = 0;
          chosen = i;
        }
      } else if (op->kind == 1) {
        Channel *c = op->target;
        if ((c->cap > 0 && c->len < c->cap) || (c->cap == 0 && c->waiting > c->len)) {
          push_item(c, op->slot);
          chosen = i;
        }
      } else if (op->kind == 2) {
        if (((Task *)op->target)->done) chosen = i;
      } else if (op->kind == 5) {
        if (((Timeline *)op->target)->now >= op->slot) chosen = i;
      } else if (op->kind == 3 ? mono_ns() >= op->slot : op->slot != 0) {
        chosen = i;
      }
    }
    if (chosen < 0) {
      if (strata_cancelled()) break;
      if (other_clocks) {
        // another clock's time may have moved: wait a little, then let the
        // compiled code ask it again
        int64_t soon = mono_ns() + 1000000;
        timed_begin("`select`");
        wait_until(first < soon ? first : soon);
        wait_end();
        chosen = -2;
        break;
      }
      if (first == INT64_MAX) {
        // a `VirtualClock`'s timer: the nearest (its time may jump to it)
        Timeline *vt = NULL;
        int64_t vd = 0;
        for (int64_t i = 0; i < n; i++) {
          Timeline *tl = ops[i].target;
          if (ops[i].kind == 5 && (!vt || ops[i].slot - tl->now < vd - vt->now)) {
            vt = tl;
            vd = ops[i].slot;
          }
        }
        if (wait_begin_v("`select`", vt, vd)) pthread_cond_wait(&chan_cond, &chan_lock);
        wait_end();
      } else {
        timed_begin("`select`");
        wait_until(first);
        wait_end();
      }
    }
  }
  for (int64_t i = 0; i < n; i++)
    if (ops[i].kind == 0) ((Channel *)ops[i].target)->waiting--;
  wake_all();
  if (chosen >= 0 || chosen == -2) {
    pthread_mutex_unlock(&chan_lock);
    return chosen;
  }
give_up:
  // cancelled while it waited: nothing happened; the values its sends held are dropped
  pthread_mutex_unlock(&chan_lock);
  for (int64_t i = 0; i < n; i++) {
    Channel *c = ops[i].target;
    if (ops[i].kind == 1 && c->drop) c->drop(ops[i].slot);
  }
  return -1;
}

// The last thread of its run is about to wait (holding `chan_lock`): if every
// thread of the run waits and none of their waits ends by itself, a virtual
// clock moves to its nearest deadline (1: look again), or the run is
// deadlocked: a panic (the program ends; a test fails, its tasks stop).
static _Bool stuck(void) {
  Run *r = my_run;
  Waiter *next = NULL;
  for (Waiter *w = r->first; w; w = w->next) {
    if (w->gen != wake_gen || w->timed) return 0;
    if (w->vt && (!next || w->vdeadline - w->vt->now < next->vdeadline - next->vt->now)) next = w;
  }
  if (next) {
    if (next->vdeadline > next->vt->now) next->vt->now = next->vdeadline;
    wake_all();
    return 1;
  }
  // what each thread waits on, and where
  size_t cap = 256, len = 0;
  char *m = malloc(cap);
  len = (size_t)snprintf(m, cap, "deadlock: every task is waiting, so none can go on:");
  for (Waiter *w = r->first; w; w = w->next) {
    char *st = w->frames ? stack_from(w->frames, w->counts, *w->runs) : strdup("");
    const char *o = w->origin ? w->origin : "";
    size_t need = strlen(st) + strlen(o) + strlen(w->what) + 64;
    if (len + need >= cap) m = realloc(m, cap = (len + need) * 2);
    len += (size_t)sprintf(m + len, "%s %s waits in %s, at %s%s%s%s", w == r->first ? "" : ";", w->task ? "a task" : (r->test ? "the test" : "main"), w->what,
                           o, *o ? " -> spawn" : "", *o && *st ? " -> " : "", *st ? st : (*o ? "" : "its start"));
    free(st);
  }
  Str *msg = str_c(m);
  free(m);
  if (r->test) {
    // the test fails with it: its tasks stop, and so does its thread
    Contained *c = r->test;
    if (!c->panicked) {
      c->panicked = 1;
      c->kind = 4;
      c->msg = msg;
      c->stack = strdup("");
      __atomic_add_fetch(&strata_cancel_any, 1, __ATOMIC_RELAXED);
    }
    for (Waiter *w = r->first; w; w = w->next)
      if (w->task) cancel_task(w->task);
    wake_all();
    wake_readers();
    return 1;
  }
  pthread_mutex_unlock(&chan_lock);
  RS->trap = NULL;
  fault_at(4 | 256, msg, NULL);
}

// After a fault caught by a trap: the tasks of every scope still open finish.
static void finish_open_scopes(void) {
  while (open_scopes) {
    TaskScope *s = open_scopes;
    for (Task *t = s->first; t; t = t->next) join(t);
    open_scopes = s->up;
    release_scope(s);
  }
}

// ---- integers ----
//
// Written in Strata in core.int (core/int.strata), as the strings are in
// core.str and the floats in core.float: these are the first stage's.
//
// An `Int` never overflows (as the interpreter's): one in [-2^62, 2^62) is
// itself, and any other is a big integer, a pointer kept in the 64-bit
// values outside that range: above it for positive ones, below for
// negative ones, so comparing a small and a big integer as 64-bit values
// is right. The generated code does small arithmetic itself and calls
// these when an operand or the result isn't small. Big integers are never
// freed (they are rare: a program that overflows).

#ifndef STRATA_CORE_RT
#define SMALL_MIN (-(INT64_C(1) << 62))
#define SMALL_MAX ((INT64_C(1) << 62) - 1)

// A number: sign (-1, 0, 1) and magnitude, 32-bit digits, least first.
typedef struct {
  int sign;
  int64_t n;
  uint32_t *d;
} Num;

// A big integer's object.
typedef struct {
  int64_t sign, n;
  uint32_t d[];
} Big;

static int is_big(int64_t v) { return v < SMALL_MIN || v > SMALL_MAX; }

static Big *big_at(int64_t v) {
  uint64_t k = v > 0 ? (uint64_t)v - ((uint64_t)1 << 62) : (uint64_t)(-(v + 1)) - ((uint64_t)1 << 62);
  return (Big *)(uintptr_t)(k << 3);
}

#ifndef STRATA_CORE_RT
// One holder more, or fewer, of a big integer (the generated code tests
// whether an `Int` is big before calling these).
void strata_int_dup(int64_t v) {
  if (is_big(v)) strata_dup(big_at(v));
}

void strata_int_drop(int64_t v) {
  if (is_big(v) && strata_release(big_at(v))) strata_free(big_at(v));
}
#endif

static uint32_t *digits(int64_t n) {
  uint32_t *d = calloc((size_t)(n > 0 ? n : 1), 4);
  if (!d) {
    fputs("fault (Limit): out of memory\n", stderr);
    exit(101);
  }
  return d;
}

// `v` as a number (its digits are the caller's to free).
static Num num_of(int64_t v) {
  Num x;
  if (is_big(v)) {
    Big *b = big_at(v);
    x.sign = (int)b->sign, x.n = b->n, x.d = digits(b->n);
    memcpy(x.d, b->d, (size_t)b->n * 4);
    return x;
  }
  uint64_t m = v < 0 ? (uint64_t)0 - (uint64_t)v : (uint64_t)v;
  x.sign = v < 0 ? -1 : v > 0, x.d = digits(2), x.n = 0;
  if (m) x.d[x.n++] = (uint32_t)m;
  if (m >> 32) x.d[x.n++] = (uint32_t)(m >> 32);
  return x;
}

static void trim(Num *x) {
  while (x->n > 0 && x->d[x->n - 1] == 0) x->n--;
  if (x->n == 0) x->sign = 0;
}

// The `Int` of a number (which it frees): small, or a new big integer.
static int64_t int_of(Num x) {
  trim(&x);
  if (x.n <= 2) {
    uint64_t m = x.n ? x.d[0] | (x.n > 1 ? (uint64_t)x.d[1] << 32 : 0) : 0;
    if (m <= (uint64_t)SMALL_MAX + (x.sign < 0)) {
      free(x.d);
      return x.sign < 0 ? (int64_t)((uint64_t)0 - m) : (int64_t)m;
    }
  }
  // an object, counted as any other (strata_alloc's are 8-byte aligned)
  Big *b = strata_alloc((int64_t)(sizeof(Big) + (size_t)x.n * 4));
  b->sign = x.sign, b->n = x.n;
  memcpy(b->d, x.d, (size_t)x.n * 4);
  free(x.d);
  uint64_t k = (uint64_t)(uintptr_t)b >> 3;
  return x.sign > 0 ? (int64_t)(((uint64_t)1 << 62) + k) : -(int64_t)(((uint64_t)1 << 62) + k) - 1;
}

#ifndef STRATA_CORE_RT
static int mag_cmp(const uint32_t *a, int64_t an, const uint32_t *b, int64_t bn) {
  if (an != bn) return an < bn ? -1 : 1;
  for (int64_t i = an - 1; i >= 0; i--)
    if (a[i] != b[i]) return a[i] < b[i] ? -1 : 1;
  return 0;
}

static Num mag_add(Num a, Num b, int sign) {
  int64_t n = (a.n > b.n ? a.n : b.n) + 1;
  Num r = {sign, n, digits(n)};
  uint64_t c = 0;
  for (int64_t i = 0; i < n; i++) {
    c += (uint64_t)(i < a.n ? a.d[i] : 0) + (i < b.n ? b.d[i] : 0);
    r.d[i] = (uint32_t)c;
    c >>= 32;
  }
  return r;
}

// |a| - |b|, with |a| >= |b|.
static Num mag_sub(Num a, Num b, int sign) {
  Num r = {sign, a.n, digits(a.n)};
  int64_t borrow = 0;
  for (int64_t i = 0; i < a.n; i++) {
    int64_t t = (int64_t)a.d[i] - (i < b.n ? b.d[i] : 0) - borrow;
    borrow = t < 0;
    r.d[i] = (uint32_t)(t + (borrow ? (INT64_C(1) << 32) : 0));
  }
  return r;
}

static Num num_add(Num a, Num b) {
  if (a.sign == 0) return mag_add(b, (Num){0, 0, NULL}, b.sign);
  if (b.sign == 0) return mag_add(a, (Num){0, 0, NULL}, a.sign);
  if (a.sign == b.sign) return mag_add(a, b, a.sign);
  int c = mag_cmp(a.d, a.n, b.d, b.n);
  if (c == 0) return (Num){0, 0, digits(1)};
  return c > 0 ? mag_sub(a, b, a.sign) : mag_sub(b, a, b.sign);
}

static Num num_mul(Num a, Num b) {
  int64_t n = a.n + b.n;
  Num r = {a.sign * b.sign, n, digits(n)};
  for (int64_t i = 0; i < a.n; i++) {
    uint64_t c = 0;
    for (int64_t j = 0; j < b.n; j++) {
      c += (uint64_t)a.d[i] * b.d[j] + r.d[i + j];
      r.d[i + j] = (uint32_t)c;
      c >>= 32;
    }
    for (int64_t k = i + b.n; c; k++) {
      c += r.d[k];
      r.d[k] = (uint32_t)c;
      c >>= 32;
    }
  }
  return r;
}

// Truncated |a| / |b| and |a| % |b| (b is not zero), bit by bit.
static void mag_divmod(Num a, Num b, Num *q, Num *r) {
  uint32_t *qd = digits(a.n), *rd = digits(b.n + 1);
  int64_t rn = 0;
  for (int64_t i = a.n * 32 - 1; i >= 0; i--) {
    // r = 2r + bit i of a (r < 2|b| fits in b.n + 1 digits)
    uint32_t carry = (a.d[i / 32] >> (i % 32)) & 1;
    for (int64_t k = 0; k < rn; k++) {
      uint32_t top = rd[k] >> 31;
      rd[k] = rd[k] << 1 | carry;
      carry = top;
    }
    if (carry) rd[rn++] = carry;
    if (mag_cmp(rd, rn, b.d, b.n) >= 0) {
      int64_t borrow = 0;
      for (int64_t k = 0; k < rn; k++) {
        int64_t t = (int64_t)rd[k] - (k < b.n ? b.d[k] : 0) - borrow;
        borrow = t < 0;
        rd[k] = (uint32_t)(t + (borrow ? (INT64_C(1) << 32) : 0));
      }
      while (rn > 0 && rd[rn - 1] == 0) rn--;
      qd[i / 32] |= (uint32_t)1 << (i % 32);
    }
  }
  *q = (Num){1, a.n, qd};
  *r = (Num){1, rn, rd};
}

// Euclidean division: the remainder is never negative (as the interpreter's).
static void num_divmod(Num a, Num b, int64_t *qv, int64_t *rv) {
  Num q, r;
  mag_divmod(a, b, &q, &r);
  q.sign = a.sign * b.sign;
  r.sign = a.sign;
  trim(&q);
  trim(&r);
  if (r.sign < 0) {
    // q -= sign(b) (toward the Euclidean quotient), r += |b|
    Num one = {1, 1, digits(1)};
    one.d[0] = 1;
    one.sign = b.sign > 0 ? -1 : 1;
    Num q2 = num_add(q, one);
    Num bb = {1, b.n, b.d};
    Num r2 = num_add(r, bb);
    free(q.d), free(r.d), free(one.d);
    q = q2, r = r2;
  }
  *qv = int_of(q);
  *rv = int_of(r);
}

typedef enum { ADD, SUB, MUL } IntOp;

static int64_t int_op(int64_t a, int64_t b, IntOp op) {
  Num x = num_of(a), y = num_of(b);
  if (op == SUB) y.sign = -y.sign;
  Num r = op == MUL ? num_mul(x, y) : num_add(x, y);
  free(x.d), free(y.d);
  return int_of(r);
}

int64_t strata_int_add(int64_t a, int64_t b) { return int_op(a, b, ADD); }
int64_t strata_int_sub(int64_t a, int64_t b) { return int_op(a, b, SUB); }
int64_t strata_int_mul(int64_t a, int64_t b) { return int_op(a, b, MUL); }

// -1, 0 or 1.
int64_t strata_int_cmp(int64_t a, int64_t b) {
  if (!is_big(a) || !is_big(b)) return a < b ? -1 : a > b;
  Big *x = big_at(a), *y = big_at(b);
  if (x->sign != y->sign) return x->sign < y->sign ? -1 : 1;
  int c = mag_cmp(x->d, x->n, y->d, y->n);
  return x->sign < 0 ? -c : c;
}

_Bool strata_int_eq(int64_t a, int64_t b) { return a == b || (is_big(a) && is_big(b) && strata_int_cmp(a, b) == 0); }

int64_t strata_int_abs(int64_t a) {
  Num x = num_of(a);
  if (x.sign < 0) x.sign = 1;
  return int_of(x);
}

int64_t strata_int_from_i64(int64_t raw);

// `a / b` for a `b` that isn't 0: what `verify` proved (`no division by
// zero`), so the compiled code calls this without the check.
int64_t strata_div_nz(int64_t a, int64_t b) {
  if (!is_big(a) && !is_big(b)) {
    // Euclidean: the remainder is never negative
    int64_t q = a / b, r = a % b;
    if (r < 0) q = b > 0 ? q - 1 : q + 1;
    return strata_int_from_i64(q);  // (-2^62 / -1 is 2^62: a big integer)
  }
  Num x = num_of(a), y = num_of(b);
  int64_t q, r;
  num_divmod(x, y, &q, &r);
  free(x.d), free(y.d);
  strata_int_drop(r);
  return q;
}

int64_t strata_div(int64_t a, int64_t b) {
  if (b == 0 && soft_fault(4, str_c("division by zero"))) return 0;
  return strata_div_nz(a, b);
}

// `a % b` for a `b` that isn't 0 (see `strata_div_nz`).
int64_t strata_mod_nz(int64_t a, int64_t b) {
  if (!is_big(a) && !is_big(b)) {
    if (b == -1) return 0;
    int64_t r = a % b;
    if (r < 0) r = b > 0 ? r + b : r - b;
    return r;
  }
  Num x = num_of(a), y = num_of(b);
  int64_t q, r;
  num_divmod(x, y, &q, &r);
  free(x.d), free(y.d);
  strata_int_drop(q);
  return r;
}

int64_t strata_mod(int64_t a, int64_t b) {
  if (b == 0 && soft_fault(4, str_c("division by zero"))) return 0;
  return strata_mod_nz(a, b);
}
#endif

#ifndef STRATA_CORE_RT
// ---- bits: `&`, `|`, `^`, `<<`, `>>` ----
//
// On two's complement, infinitely sign-extended (as Python's integers): a
// number is written in `len` words, its sign repeated in the last.

static void to_twos(Num x, uint32_t *w, int64_t len) {
  memset(w, 0, (size_t)len * 4);
  memcpy(w, x.d, (size_t)x.n * 4);
  if (x.sign < 0) {
    uint64_t carry = 1;
    for (int64_t i = 0; i < len; i++) {
      uint64_t v = (uint64_t)(uint32_t)~w[i] + carry;
      w[i] = (uint32_t)v;
      carry = v >> 32;
    }
  }
}

// The number `w` holds (it takes `w`).
static Num from_twos(uint32_t *w, int64_t len) {
  Num x = {1, len, w};
  if (len > 0 && (w[len - 1] >> 31)) {
    uint64_t carry = 1;
    for (int64_t i = 0; i < len; i++) {
      uint64_t v = (uint64_t)(uint32_t)~w[i] + carry;
      w[i] = (uint32_t)v;
      carry = v >> 32;
    }
    x.sign = -1;
  }
  return x;
}

typedef enum { AND, OR, XOR } BitOp;

static int64_t bit_op(int64_t a, int64_t b, BitOp op) {
  if (!is_big(a) && !is_big(b)) return op == AND ? (a & b) : op == OR ? (a | b) : (a ^ b);
  Num x = num_of(a), y = num_of(b);
  int64_t len = (x.n > y.n ? x.n : y.n) + 1;
  uint32_t *u = digits(len), *v = digits(len);
  to_twos(x, u, len);
  to_twos(y, v, len);
  for (int64_t i = 0; i < len; i++) u[i] = op == AND ? (u[i] & v[i]) : op == OR ? (u[i] | v[i]) : (u[i] ^ v[i]);
  free(x.d), free(y.d), free(v);
  return int_of(from_twos(u, len));
}

int64_t strata_int_and(int64_t a, int64_t b) { return bit_op(a, b, AND); }
int64_t strata_int_or(int64_t a, int64_t b) { return bit_op(a, b, OR); }
int64_t strata_int_xor(int64_t a, int64_t b) { return bit_op(a, b, XOR); }

// The shift `n` as a count of bits; a negative one is a fault (-1 after a
// contained one).
static int64_t shift_of(int64_t n, const char *op) {
  if (n < 0 || (is_big(n) && big_at(n)->sign < 0)) {
    soft_fault(4, strata_concat_owned(str_c(op), str_c(" by a negative number of bits")));
    return -1;
  }
  return is_big(n) ? INT64_MAX : n;
}

// `a << n`: `a` times 2^n.
int64_t strata_shl(int64_t a, int64_t n) {
  int64_t k = shift_of(n, "`<<`");
  if (k < 0 || a == 0) return 0;
  if (k > ((int64_t)1 << 32) && soft_fault(6, str_c("`<<` by more than 2^32 bits"))) return 0;
  if (!is_big(a) && k < 62) {
    int64_t lim = (int64_t)1 << (62 - k);
    if (a > -lim && a < lim) return a * ((int64_t)1 << k);
  }
  Num x = num_of(a);
  int64_t words = k / 32, bits = k % 32, len = x.n + words + 1;
  uint32_t *d = digits(len);
  for (int64_t i = 0; i < x.n; i++) {
    uint64_t v = (uint64_t)x.d[i] << bits;
    d[i + words] |= (uint32_t)v;
    d[i + words + 1] |= (uint32_t)(v >> 32);
  }
  Num r = {x.sign, len, d};
  free(x.d);
  return int_of(r);
}

// `a >> n`: `a` divided by 2^n, rounded down (as `/` by a power of two).
int64_t strata_shr(int64_t a, int64_t n) {
  int64_t k = shift_of(n, "`>>`");
  if (!is_big(a)) return a >> (k > 63 ? 63 : k);
  Num x = num_of(a);
  int64_t words = k / 32, bits = k % 32;
  if (words >= x.n) {
    int neg = x.sign < 0;
    free(x.d);
    return neg ? -1 : 0;
  }
  // on the two's complement, so a negative number rounds down
  int64_t len = x.n + 1;
  uint32_t *w = digits(len);
  to_twos(x, w, len);
  uint32_t fill = (w[len - 1] >> 31) ? 0xffffffffu : 0;
  int64_t out = len - words;
  uint32_t *d = digits(out);
  for (int64_t i = 0; i < out; i++) {
    uint64_t lo = w[i + words], hi = i + words + 1 < len ? w[i + words + 1] : fill;
    d[i] = (uint32_t)(((hi << 32) | lo) >> bits);
  }
  free(x.d), free(w);
  return int_of(from_twos(d, out));
}

// A big integer's decimal digits: chunks of nine, least first, divided out.
static Str *big_show(int64_t v) {
  Num x = num_of(v);
  uint32_t *chunks = digits(x.n * 2 + 1);
  int64_t nc = 0;
  while (x.n > 0) {
    uint64_t rem = 0;
    for (int64_t i = x.n - 1; i >= 0; i--) {
      uint64_t cur = rem << 32 | x.d[i];
      x.d[i] = (uint32_t)(cur / 1000000000);
      rem = cur % 1000000000;
    }
    while (x.n > 0 && x.d[x.n - 1] == 0) x.n--;
    chunks[nc++] = (uint32_t)rem;
  }
  char *out = malloc((size_t)nc * 9 + 2), *p = out;
  if (v < 0) *p++ = '-';
  p += sprintf(p, "%u", nc ? chunks[nc - 1] : 0);
  for (int64_t i = nc - 2; i >= 0; i--) p += sprintf(p, "%09u", chunks[i]);
  Str *s = str_c(out);
  free(out), free(chunks), free(x.d);
  return s;
}

// The digits of an integer literal too large to be small.
int64_t strata_int_lit(Str *s) {
  Num x = {1, 0, digits(s->len / 9 + 2)};
  int neg = s->len > 0 && s->bytes[0] == '-';
  for (int64_t i = neg; i < s->len; i++) {
    uint64_t c = (uint64_t)(s->bytes[i] - '0');
    for (int64_t k = 0; k < x.n; k++) {
      c += (uint64_t)x.d[k] * 10;
      x.d[k] = (uint32_t)c;
      c >>= 32;
    }
    if (c) x.d[x.n++] = (uint32_t)c;
  }
  x.sign = x.n ? (neg ? -1 : 1) : 0;
  return int_of(x);
}
#endif

Str *strata_show_int(int64_t n);

// `float(n)`: the nearest Float (Rust's `to_f64`: rounded to nearest, ties even).
double strata_int_to_float(int64_t v) {
  if (!is_big(v)) return (double)v;
  Num x = num_of(v);
  // the top 64 bits, the rest folded into the lowest (sticky), then scaled
  int64_t bits = x.n * 32;
  while (bits > 0 && !((x.d[(bits - 1) / 32] >> ((bits - 1) % 32)) & 1)) bits--;
  uint64_t top = 0;
  int sticky = 0;
  for (int64_t i = bits - 1, k = 0; i >= 0; i--, k++) {
    int bit = (x.d[i / 32] >> (i % 32)) & 1;
    if (k < 64) top = top << 1 | (uint64_t)bit;
    else sticky |= bit;
  }
  int64_t shift = bits > 64 ? bits - 64 : 0;
  if (sticky) top |= 1;
  double d = ldexp((double)top, (int)shift);
  free(x.d);
  if (!isfinite(d)) {
    Str *m = strata_show_int(v);
    soft_fault(4, strata_concat_owned(m, str_c(" is too large for a Float")));
    return 0.0;
  }
  return x.sign < 0 ? -d : d;
}

double strata_fmod(double a, double b) {
  double r = fmod(a, b);
  if (r < 0) r += fabs(b);
  return r;
}

#ifndef STRATA_CORE_RT
Str *strata_show_int(int64_t n) {
  if (is_big(n)) return big_show(n);
  char buf[32];
  return str_c((snprintf(buf, sizeof buf, "%lld", (long long)n), buf));
}

// A 64-bit integer as an `Int` (a big integer when it isn't small).
int64_t strata_int_from_i64(int64_t raw) {
  if (!is_big(raw)) return raw;
  uint64_t m = raw < 0 ? (uint64_t)0 - (uint64_t)raw : (uint64_t)raw;
  Num x = {raw < 0 ? -1 : 1, 0, digits(2)};
  x.d[x.n++] = (uint32_t)m;
  if (m >> 32) x.d[x.n++] = (uint32_t)(m >> 32);
  return int_of(x);
}
#endif
#else
// (core.int's, through core.rt: the integer functions, below too)
void strata_int_dup(int64_t v);
void strata_int_drop(int64_t v);
Str *strata_show_int(int64_t n);
int64_t strata_int_from_i64(int64_t raw);
#endif

// ---- sized integers (`I8` … `U64`) ----
//
// Compiled inline but for these: an operation is `Int`'s, then the result
// modulo 2^bits. The generated code passes a signed type's value sign-extended
// to 64 bits, an unsigned one's zero-extended, and truncates the result.

#ifndef STRATA_CORE_RT
// Euclidean `/` and `%` (the remainder is never negative), as `Int`'s.
int64_t strata_word_div(int64_t a, int64_t b) {
  if (b == 0 && soft_fault(4, str_c("division by zero"))) return 0;
  if (b == 0) return 0;
  if (b == -1) return (int64_t)((uint64_t)0 - (uint64_t)a);
  int64_t q = a / b, r = a % b;
  if (r < 0) q = b > 0 ? q - 1 : q + 1;
  return q;
}

int64_t strata_word_mod(int64_t a, int64_t b) {
  if (b == 0 && soft_fault(4, str_c("division by zero"))) return 0;
  if (b == 0 || b == -1) return 0;
  int64_t r = a % b;
  if (r < 0) r = b > 0 ? r + b : r - b;
  return r;
}

uint64_t strata_uword_div(uint64_t a, uint64_t b) {
  if (b == 0 && soft_fault(4, str_c("division by zero"))) return 0;
  return b == 0 ? 0 : a / b;
}

uint64_t strata_uword_mod(uint64_t a, uint64_t b) {
  if (b == 0 && soft_fault(4, str_c("division by zero"))) return 0;
  return b == 0 ? 0 : a % b;
}

// A shift of a signed type by a negative number of bits, as `Int`'s.
void strata_word_shift_fault(Str *op) {
  soft_fault(4, strata_concat_owned((strata_dup(op), op), str_c(" by a negative number of bits")));
}

int64_t strata_int_bits(int64_t v);

// `to_u32(n)` and the like: `n`'s low 64 bits, when `n` is in lo ..= hi
// (both `Int`s); else a fault naming the type.
int64_t strata_int_fit(int64_t n, int64_t lo, int64_t hi, Str *ty) {
  if (strata_int_cmp(lo, n) > 0 || strata_int_cmp(n, hi) > 0) {
    Str *m = strata_concat_owned(strata_concat_owned(strata_concat_owned(str_c("`"), strata_show_int(n)), str_c("` doesn't fit in `")), (strata_dup(ty), ty));
    soft_fault(4, strata_concat_owned(m, str_c("`")));
    return 0;
  }
  return strata_int_bits(n);
}
#endif

// ---- foreign calls (`extern c`) ----
//
// What the generated code converts a value with, on its way to C and back.

#ifndef STRATA_CORE_RT
// An `Int` as its low 64 bits: an `I64` or a `U64` (two's complement), which
// its refinement already keeps in range.
int64_t strata_int_bits(int64_t v) {
  if (!is_big(v)) return v;
  Big *b = big_at(v);
  uint64_t m = b->n ? b->d[0] | (b->n > 1 ? (uint64_t)b->d[1] << 32 : 0) : 0;
  return b->sign < 0 ? (int64_t)((uint64_t)0 - m) : (int64_t)m;
}

// A `uint64_t` from C as an `Int`.
int64_t strata_int_from_u64(uint64_t raw) {
  if (raw <= (uint64_t)SMALL_MAX) return (int64_t)raw;
  Num x = {1, 0, digits(2)};
  x.d[x.n++] = (uint32_t)raw;
  x.d[x.n++] = (uint32_t)(raw >> 32);
  return int_of(x);
}
#endif

// A `String` for C: a NUL-terminated copy (freed with `strata_cstr_free`
// after the call). C can't see past a NUL, so a string holding one is a fault.
char *strata_cstr(Str *s, Str *callee) {
  if (memchr(s->bytes, 0, (size_t)s->len)) {
    Str *m = strata_concat_owned(strata_concat_owned(str_c("a string passed to `"), (strata_dup(callee), callee)), str_c("` holds a NUL character, which C would read as its end"));
    soft_fault(4, m);
    char *e = malloc(1);  // (contained: the call is still made, then the code leaves)
    if (e) *e = 0;
    return e;
  }
  char *c = malloc((size_t)s->len + 1);
  if (!c) {
    fputs("fault (Limit): out of memory\n", stderr);
    exit(101);
  }
  memcpy(c, s->bytes, (size_t)s->len);
  c[s->len] = 0;
  return c;
}

void strata_cstr_free(char *c) { free(c); }

// A string C returned, copied (C keeps its own); NULL is a fault.
Str *strata_str_from_c(const char *c, Str *callee) {
  if (!c) {
    Str *m = strata_concat_owned(strata_concat_owned(str_c("`"), (strata_dup(callee), callee)), str_c("` returned NULL for a `String`"));
    soft_fault(4, m);
    return NO_STR;
  }
  return str_c(c);
}

// ---- generated values (`mock`) ----
//
// As the interpreter's generators (src/generate.rs): the generated code
// picks with these, from bounds the compiler read in the refinements. The
// numbers are a fixed sequence (splitmix64 from 7), not the interpreter's.

// (`rng`, the state, is defined above)


// The generators: core.gen's when core.rt is linked in (`STRATA_CORE_RT`),
// their state core.test's (`RS->rng`).
#ifndef STRATA_CORE_RT
static uint64_t next_random(void) {
  uint64_t z = (RS->rng += UINT64_C(0x9E3779B97F4A7C15));
  z = (z ^ (z >> 30)) * UINT64_C(0xBF58476D1CE4E5B9);
  z = (z ^ (z >> 27)) * UINT64_C(0x94D049BB133111EB);
  return z ^ (z >> 31);
}

// From `lo` to `hi`, both included (as 64-bit values).
int64_t strata_random(int64_t lo, int64_t hi) {
  if (hi <= lo) return lo;
  uint64_t span = (uint64_t)hi - (uint64_t)lo + 1;
  return span ? (int64_t)((uint64_t)lo + next_random() % span) : (int64_t)next_random();
}

double strata_random_unit(void) { return (double)(next_random() >> 11) * 0x1.0p-53; }

// One of `k` intervals (pairs, open ends already brought in), then its low
// end, its high end, 0, or anything in it (boundary values find bugs).
static int64_t pick_int(const int64_t *pairs, int64_t k) {
  if (k == 0) return 0;
  int64_t i = strata_random(0, k - 1), lo = pairs[2 * i], hi = pairs[2 * i + 1];
  switch (strata_random(0, 9)) {
  case 0:
    return lo;
  case 1:
    return hi;
  case 2:
    if (lo <= 0 && 0 <= hi) return 0;
    // fall through
  default:
    return strata_random(lo, hi);
  }
}

int64_t strata_gen_int(const int64_t *pairs, int64_t k) { return strata_int_from_i64(pick_int(pairs, k)); }

// A Float: a whole number from the intervals, and a fraction when the next
// integer is still allowed (`raw`, the intervals as the refinements say).
double strata_gen_float(const int64_t *pairs, int64_t k, const int64_t *raw, int64_t rk) {
  int64_t whole = pick_int(pairs, k);
  double frac = strata_random_unit();
  int inside = 0;
  for (int64_t i = 0; i < rk && whole < INT64_MAX; i++)
    if (raw[2 * i] <= whole + 1 && whole + 1 <= raw[2 * i + 1]) inside = 1;
  return inside ? (double)whole + frac : (double)whole;
}

int64_t strata_gen_len(const int64_t *pairs, int64_t k) {
  if (k == 0) return 0;
  int64_t i = strata_random(0, k - 1), lo = pairs[2 * i] > 0 ? pairs[2 * i] : 0, hi = pairs[2 * i + 1] > 0 ? pairs[2 * i + 1] : 0;
  return strata_random(lo, hi > lo ? hi : lo);
}

Str *strata_gen_string(int64_t n) {
  static const char CH[] = "abcdefghijklmnopqrstuvwxyz0123456789 _-";
  Str *s = str_alloc(n);
  for (int64_t i = 0; i < n; i++) s->bytes[i] = CH[strata_random(0, (int64_t)sizeof CH - 2)];
  return s;
}
#endif

_Noreturn void strata_overflow(void) {
  strata_fault(4, str_c("integer overflow"));
}

// Float show and parse are the C library's, with core.rt too: core.float's
// exact versions over big numbers give the same text but took about 15
// times as long (`snprintf` and `strtod` are correctly rounded).

// The shortest decimal digits that read back as x (x is not 0): x is
// 0.DIGITS * 10^kk. Returns whether x is negative.
static int shortest(double x, char digits[32], int *kk) {
  // (x rounded to 15 digits, without its trailing zeros, is the shortest
  // that reads back, when one of 15 digits or fewer does: such a number is
  // within half a unit of the 15th digit of x; else 16 digits, else 17.
  // Not for a subnormal x, less precise: the fewest digits, one by one)
  char buf[64];
  if (fabs(x) < DBL_MIN) {
    for (int prec = 0; prec < 17; prec++) {
      snprintf(buf, sizeof buf, "%.*e", prec, x);
      if (strtod(buf, NULL) == x) break;
    }
  } else if (snprintf(buf, sizeof buf, "%.14e", x), strtod(buf, NULL) != x) {
    snprintf(buf, sizeof buf, "%.15e", x);
    if (strtod(buf, NULL) != x) snprintf(buf, sizeof buf, "%.16e", x);
  }
  // buf is [-]d[.ddd]e[+-]XX: split into its digits and exponent
  char *p = buf, *out = digits;
  int neg = *p == '-';
  if (neg) p++;
  while (*p != 'e') {
    if (*p != '.') *out++ = *p;
    p++;
  }
  // (the trailing zeros: 1.00000000000000e+00 is 1)
  while (out > digits + 1 && out[-1] == '0') out--;
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

#ifndef STRATA_CORE_RT
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

#ifndef STRATA_CORE_RT
// A string as `Debug` writes it: a Strata string literal, escaped as the
// printer escapes (\n, \t, \", and a backslash that would start one).
Str *strata_debug_str(Str *s) {
  Str *o = str_alloc(s->len * 2 + 2);
  char *p = o->bytes;
  *p++ = '"';
  for (int64_t i = 0; i < s->len; i++) {
    char c = s->bytes[i];
    char n = i + 1 < s->len ? s->bytes[i + 1] : 0;
    if (c == '\n') { *p++ = '\\'; *p++ = 'n'; }
    else if (c == '\t') { *p++ = '\\'; *p++ = 't'; }
    else if (c == '"') { *p++ = '\\'; *p++ = '"'; }
    else if (c == '\\' && (i + 1 == s->len || n == 'n' || n == 't' || n == '"' || n == '\\')) { *p++ = '\\'; *p++ = '\\'; }
    else *p++ = c;
  }
  *p++ = '"';
  o->len = p - o->bytes;
  return o;
}
#endif

// A float operation's result must be finite (no NaN or infinity).
double strata_finite(double r, double a, double b, Str *op) {
  if (isfinite(r)) return r;
  // (`op` is the caller's: not consumed)
  Str *head = strata_concat_owned(show_float_plain(a), str_c(" ")), *m = strata_concat(head, op);
  str_drop(head);
  m = strata_concat_owned(m, str_c(" "));
  soft_fault(4, strata_concat_owned(strata_concat_owned(m, show_float_plain(b)), str_c(" is not a finite number")));
  return 0.0;
}

// `floor`, `ceil`, `round` of `x` gave `r`, a whole number: as an Int
// (exactly, a big integer when it isn't small).
int64_t strata_float_to_int(double r, double x) {
  if (!isfinite(r) && soft_fault(4, strata_concat_owned(show_float_plain(x), str_c(" is not a whole number")))) return 0;
  if (r >= -4611686018427387904.0 && r < 4611686018427387904.0) return (int64_t)r;
  int e;
  double m = frexp(fabs(r), &e);  // |r| = m * 2^e, 0.5 <= m < 1
  uint64_t mant = (uint64_t)ldexp(m, 53);
  int shift = e - 53;
  int64_t n = (shift + 31) / 32 + 3;
  Num v = {r < 0 ? -1 : 1, n, digits(n)};
  for (int b = 0; b < 53; b++)
    if ((mant >> b) & 1) {
      int64_t at = b + shift;
      v.d[at / 32] |= (uint32_t)1 << (at % 32);
    }
  return int_of(v);
}

double strata_sqrt(double x) {
  if (x >= 0) return sqrt(x);
  soft_fault(4, strata_concat_owned(str_c("sqrt of negative number "), show_float_plain(x)));
  return 0.0;
}
#endif

// ---- lists ----
//
// Written in Strata in core.list (core/list.strata), which replaces these
// when core.rt is linked in (`STRATA_CORE_RT`); `buf_new` and `append` stay
// for the host functions' lists.
//
// A list is a header (its length, a buffer, and where in the buffer it starts),
// the buffer the elements as 64-bit slots. Lists never change: `push` writes
// in place only past the end of what every list sharing the buffer has used,
// so `out = push(out, x)` in a loop is amortized O(1). The generated code
// reads `len` and the elements directly (the layout is in src/llvm.rs).
//
// Headers and buffers are counted objects: a header holds its buffer, and a
// buffer holds its elements up to `used`. These functions borrow the lists
// they are given; when the elements are counted objects (`counted`), each
// one copied into a new buffer is held once more. The generated code drops
// lists (it knows the elements' types).

typedef struct {
  int64_t cap, used;
  int64_t data[];
} Buf;

typedef struct {
  int64_t len;
  Buf *buf;
  int64_t off;
} List;

static Buf *buf_new(int64_t cap) {
  if (cap < 4) cap = 4;
  Buf *b = strata_alloc((int64_t)sizeof(Buf) + 8 * cap);
  b->cap = cap;
  b->used = 0;
  return b;
}

#ifndef STRATA_CORE_RT
List *strata_list_new(int64_t cap) {
  List *l = strata_alloc(sizeof(List));
  l->len = 0;
  l->buf = buf_new(cap);
  l->off = 0;
  return l;
}

void strata_int_dup(int64_t v);

// Hold what `n` copied slots hold: `kind` 0 nothing, 1 objects, 2 integers
// (the big ones are objects).
static void dup_slots(int64_t *xs, int64_t n, int64_t kind) {
  if (kind == 1)
    for (int64_t i = 0; i < n; i++) strata_dup((void *)(intptr_t)xs[i]);
  else if (kind == 2)
    for (int64_t i = 0; i < n; i++) strata_int_dup(xs[i]);
}

// `xs` with `x` after it (`x` is handed over).
List *strata_list_push(List *xs, int64_t x, int64_t counted) {
  Buf *b = xs->buf;
  int64_t end = xs->off + xs->len;
  List *l = strata_alloc(sizeof(List));
  if (end == b->used && end < b->cap) {
    // nothing else has used the slot after this list's end
    b->data[end] = x;
    b->used++;
    strata_dup(b);
    *l = (List){xs->len + 1, b, xs->off};
    return l;
  }
  Buf *n = buf_new(xs->len * 2 + 1);
  memcpy(n->data, b->data + xs->off, (size_t)xs->len * 8);
  dup_slots(n->data, xs->len, counted);
  n->data[xs->len] = x;
  n->used = xs->len + 1;
  *l = (List){xs->len + 1, n, 0};
  return l;
}

// Whether a list can be changed in place: its header and its buffer held
// once (by what is about to be replaced by the change), the buffer all its own.
_Bool strata_list_owned(List *xs) {
  return strata_unique(xs) && strata_unique(xs->buf) && xs->off == 0 && xs->len == xs->buf->used;
}

// In an owned list (`strata_list_owned`): element `i` replaced by `x` (held
// already); the old one returned, for the caller to drop.
int64_t strata_list_put(List *xs, int64_t i, int64_t x) {
  int64_t old = xs->buf->data[i];
  xs->buf->data[i] = x;
  return old;
}

// In an owned list: `x` (held already) inserted before element `i`.
void strata_list_insert(List *xs, int64_t i, int64_t x) {
  Buf *b = xs->buf;
  if (b->used == b->cap) {
    Buf *n = buf_new(b->cap * 2 + 1);
    memcpy(n->data, b->data, (size_t)b->used * 8);
    n->used = b->used;
    strata_free(b);
    xs->buf = b = n;
  }
  memmove(b->data + i + 1, b->data + i, (size_t)(b->used - i) * 8);
  b->data[i] = x;
  b->used++;
  xs->len++;
}

// In an owned list: element `i` taken out (the rest moved down) and
// returned, for the caller to drop.
int64_t strata_list_remove_at(List *xs, int64_t i) {
  Buf *b = xs->buf;
  int64_t old = b->data[i];
  memmove(b->data + i, b->data + i + 1, (size_t)(b->used - i - 1) * 8);
  b->used--;
  xs->len--;
  return old;
}
#else
// (core.rt's: the list functions, below too; `append` is the host
// functions')
List *strata_list_new(int64_t cap);
List *strata_list_push(List *xs, int64_t x, int64_t counted);
#endif

// Add `x` to a list nothing else holds yet (one the runtime is making).
static List *append(List *l, int64_t x) {
  Buf *b = l->buf;
  if (b->used == b->cap) {
    Buf *n = buf_new(b->cap * 2);
    memcpy(n->data, b->data, (size_t)b->used * 8);
    n->used = b->used;
    strata_free(b);
    l->buf = b = n;
  }
  b->data[b->used++] = x;
  l->len++;
  return l;
}

// ---- dictionaries and sets ----
//
// A `Dict[K, V]` or a `Set[A]` is a hash table: a block { tag 0, entries,
// index } laid out as the tuple `(List[E], List[Int])` the generated code
// takes it for (so it is held, dropped and copied as one), E a dict's
// `(K, V)` pairs or a set's elements. A new entry goes after the others
// and a removal moves the last entry into the gap (so a removal is O(1)),
// so `for`, `len`, `items` and showing one are the entries list's; that
// order is the table's, not the language's (nothing should assume one),
// and the same whether a table was changed in place or copied. A table
// copied to be changed (`insert`, `remove` on a table held elsewhere) is
// the new table's alone (its entries, index and their buffers held once),
// so the changes after it are in place. The index is open addressing
// with linear probing over a power of two of slots, at most half of them
// used: a used slot holds `tag << 32 | (entry + 1)`, the tag 30 bits of the
// key's hash (most other keys of the slot are told apart without comparing
// them), 0 an empty one. A table of up to TBL_SMALL entries has no index
// (the shared empty list) and is searched in order. Keys are hashed and
// compared by functions the generated code passes, on 64-bit slots: the
// key type's structural hash and its order (`cmp`, 0 for equal keys).
// Hashing is deterministic (no seed), so a program does the same on every
// run. These functions borrow the tables and values they are given; a new
// table holds its entries (`kind`, as the list functions': 0 nothing to
// hold, 1 objects, 2 integers).

typedef int64_t (*TblHash)(int64_t);
typedef int64_t (*TblCmp)(int64_t, int64_t);

typedef struct {
  int64_t tag;
  List *es, *idx;
} Tbl;

#define TBL_SMALL 8

// The shared empty list and table: a count of 0, never counted nor freed.
static struct {
  int64_t count, cap, used;
} tbl_no_buf = {0, 0, 0};
static struct {
  int64_t count, len;
  void *buf;
  int64_t off;
} tbl_no_list = {0, 0, &tbl_no_buf.cap, 0};
static struct {
  int64_t count, tag;
  void *es, *idx;
} tbl_none = {0, 0, &tbl_no_list.len, &tbl_no_list.len};
#define NO_LIST ((List *)&tbl_no_list.len)

static int64_t *tbl_data(List *l) { return l->buf->data + l->off; }

static int64_t tbl_key(int64_t e, int64_t dict) { return dict ? ((int64_t *)(intptr_t)e)[1] : e; }

// A hash spread over all 64 bits (the low ones pick the slot, the high ones
// are the tag).
static uint64_t tbl_mix(int64_t h) {
  uint64_t m = (uint64_t)h;
  m ^= m >> 33;
  m *= 0xff51afd7ed558ccdull;
  m ^= m >> 33;
  m *= 0xc4ceb9fe1a85ec53ull;
  m ^= m >> 33;
  return m;
}

static int64_t tbl_tag(uint64_t m) { return (int64_t)((m >> 32) & 0x3FFFFFFF); }

// The number of slots for `n` entries.
static int64_t tbl_cap(int64_t n) {
  int64_t c = 16;
  while (c < 2 * n) c *= 2;
  return c;
}

static void tbl_hold(int64_t *xs, int64_t n, int64_t kind) {
  if (kind == 1)
    for (int64_t i = 0; i < n; i++) strata_dup((void *)(intptr_t)xs[i]);
  else if (kind == 2)
    for (int64_t i = 0; i < n; i++) strata_int_dup(xs[i]);
}

// Entry `e`, of mixed hash `m`, in the first free slot from its own.
static void tbl_place(int64_t *s, int64_t cap, uint64_t m, int64_t e) {
  uint64_t i = m & (uint64_t)(cap - 1);
  while (s[i]) i = (i + 1) & (uint64_t)(cap - 1);
  s[i] = tbl_tag(m) << 32 | (e + 1);
}

static List *tbl_slots(int64_t cap) {
  List *l = strata_list_new(cap);
  memset(l->buf->data, 0, (size_t)cap * 8);
  l->buf->used = l->len = cap;
  return l;
}

// A new index of the `n` entries `d` (distinct keys), or none for a small
// table.
static List *tbl_index(int64_t *d, int64_t n, TblHash h, int64_t dict) {
  if (n <= TBL_SMALL) return NO_LIST;
  int64_t cap = tbl_cap(n);
  List *l = tbl_slots(cap);
  for (int64_t e = 0; e < n; e++) tbl_place(l->buf->data, cap, tbl_mix(h(tbl_key(d[e], dict))), e);
  return l;
}

// Where key `k` is among the `n` entries `d` indexed by `idx`, or -1.
static int64_t tbl_search(int64_t *d, int64_t n, List *idx, int64_t k, TblHash h, TblCmp c, int64_t dict) {
  if (idx->len == 0) {
    for (int64_t e = 0; e < n; e++)
      if (c(tbl_key(d[e], dict), k) == 0) return e;
    return -1;
  }
  int64_t cap = idx->len, *s = tbl_data(idx);
  uint64_t m = tbl_mix(h(k));
  int64_t tag = tbl_tag(m);
  for (uint64_t i = m & (uint64_t)(cap - 1);; i = (i + 1) & (uint64_t)(cap - 1)) {
    int64_t v = s[i];
    if (!v) return -1;
    if (v >> 32 == tag) {
      int64_t e = (v & 0xFFFFFFFF) - 1;
      if (c(tbl_key(d[e], dict), k) == 0) return e;
    }
  }
}

// A copy of an index, its own (none stays none).
static List *tbl_index_copy(List *idx) {
  if (idx->len == 0) return NO_LIST;
  List *l = tbl_slots(idx->len);
  memcpy(l->buf->data, tbl_data(idx), (size_t)idx->len * 8);
  return l;
}

// The slot of `s` (`cap` of them) that holds entry `e`, whose key's mixed
// hash is `m`.
static uint64_t tbl_slot_of(int64_t *s, int64_t cap, uint64_t m, int64_t e) {
  uint64_t i = m & (uint64_t)(cap - 1);
  while ((s[i] & 0xFFFFFFFF) != e + 1) i = (i + 1) & (uint64_t)(cap - 1);
  return i;
}

// Slot `i` emptied, the slots after it in its run moved back where a search
// still finds them (linear probing's deletion, no tombstones); `d` the
// entries, for their keys' home slots.
static void tbl_unplace(int64_t *s, int64_t cap, uint64_t i, int64_t *d, TblHash h, int64_t dict) {
  uint64_t mask = (uint64_t)(cap - 1), j = i;
  for (;;) {
    j = (j + 1) & mask;
    if (!s[j]) break;
    uint64_t home = tbl_mix(h(tbl_key(d[(s[j] & 0xFFFFFFFF) - 1], dict))) & mask;
    // (s[j] may move to i when its home isn't cyclically in (i, j])
    if (i < j ? home <= i || home > j : home <= i && home > j) {
      s[i] = s[j];
      i = j;
    }
  }
  s[i] = 0;
}

static Tbl *tbl_new(List *es, List *idx) {
  Tbl *t = strata_alloc(sizeof(Tbl));
  *t = (Tbl){0, es, idx};
  return t;
}

// The empty table (shared).
Tbl *strata_tbl_empty(void) { return (Tbl *)&tbl_none.tag; }

// Where key `k` is in `t` (its entry's index), or -1.
int64_t strata_tbl_find(Tbl *t, int64_t k, TblHash h, TblCmp c, int64_t dict) {
  return tbl_search(tbl_data(t->es), t->es->len, t->idx, k, h, c, dict);
}

// A table of the `n` entries `d`, in order, a key once: a set keeps the
// first of equal elements, a dict the last pair of a key, where the key
// first came.
static Tbl *tbl_of(int64_t *d, int64_t n, TblHash h, TblCmp c, int64_t dict, int64_t kind) {
  if (n == 0) return strata_tbl_empty();
  int64_t *pos = malloc((size_t)n * 8), m = 0, cap = 0, *s = NULL;
  if (n <= TBL_SMALL) {
    for (int64_t i = 0; i < n; i++) {
      int64_t k = tbl_key(d[i], dict), j = -1;
      for (int64_t e = 0; e < m && j < 0; e++)
        if (c(tbl_key(d[pos[e]], dict), k) == 0) j = e;
      if (j < 0) pos[m++] = i;
      else if (dict) pos[j] = i;
    }
  } else {
    cap = tbl_cap(n);
    s = calloc((size_t)cap, 8);
    for (int64_t i = 0; i < n; i++) {
      int64_t k = tbl_key(d[i], dict);
      uint64_t mm = tbl_mix(h(k));
      int64_t tag = tbl_tag(mm);
      for (uint64_t at = mm & (uint64_t)(cap - 1);; at = (at + 1) & (uint64_t)(cap - 1)) {
        int64_t v = s[at];
        if (!v) {
          s[at] = tag << 32 | (m + 1);
          pos[m++] = i;
          break;
        }
        if (v >> 32 == tag) {
          int64_t e = (v & 0xFFFFFFFF) - 1;
          if (c(tbl_key(d[pos[e]], dict), k) == 0) {
            if (dict) pos[e] = i;
            break;
          }
        }
      }
    }
  }
  List *es = strata_list_new(m);
  int64_t *o = es->buf->data;
  for (int64_t e = 0; e < m; e++) o[e] = d[pos[e]];
  es->buf->used = es->len = m;
  tbl_hold(o, m, kind);
  free(pos);
  List *idx;
  if (m <= TBL_SMALL) idx = NO_LIST;
  else if (tbl_cap(m) == cap) {
    idx = tbl_slots(cap);
    memcpy(idx->buf->data, s, (size_t)cap * 8);
  } else idx = tbl_index(o, m, h, dict);
  free(s);
  return tbl_new(es, idx);
}

// `set(xs)`, `dict(pairs)`.
Tbl *strata_tbl_from(List *xs, TblHash h, TblCmp c, int64_t dict, int64_t kind) {
  return tbl_of(tbl_data(xs), xs->len, h, c, dict, kind);
}

// `insert`: `x` (an element, a dict's pair) in place of the entry of its key,
// else after the others; a set keeps an element it has.
Tbl *strata_tbl_insert(Tbl *t, int64_t x, TblHash h, TblCmp c, int64_t dict, int64_t kind) {
  int64_t n = t->es->len, *d = tbl_data(t->es), k = tbl_key(x, dict);
  int64_t e = tbl_search(d, n, t->idx, k, h, c, dict);
  if (e >= 0 && !dict) {
    strata_dup(t);
    return t;
  }
  if (e >= 0) {
    List *es = strata_list_new(n);
    memcpy(es->buf->data, d, (size_t)n * 8);
    es->buf->data[e] = x;
    es->buf->used = es->len = n;
    tbl_hold(es->buf->data, n, kind);
    return tbl_new(es, tbl_index_copy(t->idx));
  }
  // (a copy of the entries with room to grow, not `t`'s buffer extended:
  // the new table is then its own, changed in place from now on)
  tbl_hold(&x, 1, kind);
  List *es = strata_list_new(2 * (n + 1)), *idx;
  memcpy(es->buf->data, d, (size_t)n * 8);
  es->buf->data[n] = x;
  es->buf->used = es->len = n + 1;
  tbl_hold(es->buf->data, n, kind);
  if (n + 1 <= TBL_SMALL) idx = NO_LIST;
  else if (t->idx->len == 0 || 2 * (n + 1) > t->idx->len) idx = tbl_index(tbl_data(es), n + 1, h, dict);
  else {
    int64_t cap = t->idx->len;
    idx = tbl_slots(cap);
    memcpy(idx->buf->data, tbl_data(t->idx), (size_t)cap * 8);
    tbl_place(idx->buf->data, cap, tbl_mix(h(k)), n);
  }
  return tbl_new(es, idx);
}

// `remove`: the entry of key `k` taken out, the last one in its place (as
// `strata_tbl_remove_at` does in place).
Tbl *strata_tbl_remove(Tbl *t, int64_t k, TblHash h, TblCmp c, int64_t dict, int64_t kind) {
  int64_t n = t->es->len, *d = tbl_data(t->es);
  int64_t e = tbl_search(d, n, t->idx, k, h, c, dict);
  if (e < 0) {
    strata_dup(t);
    return t;
  }
  if (n == 1) return strata_tbl_empty();
  List *es = strata_list_new(n - 1);
  memcpy(es->buf->data, d, (size_t)(n - 1) * 8);
  if (e < n - 1) es->buf->data[e] = d[n - 1];
  es->buf->used = es->len = n - 1;
  tbl_hold(es->buf->data, n - 1, kind);
  return tbl_new(es, tbl_index(es->buf->data, n - 1, h, dict));
}

// A set's `union` (0: `a`'s elements, then those of `b` not in `a`),
// `intersection` (1) or `difference` (2: `a`'s not in `b`).
Tbl *strata_tbl_combine(Tbl *a, Tbl *b, int64_t op, TblHash h, TblCmp c, int64_t kind) {
  int64_t na = a->es->len, nb = b->es->len, *da = tbl_data(a->es), *db = tbl_data(b->es);
  int64_t *d = malloc((size_t)(na + nb + 1) * 8), n = 0;
  if (op == 0) {
    memcpy(d, da, (size_t)na * 8);
    n = na;
    for (int64_t i = 0; i < nb; i++)
      if (tbl_search(da, na, a->idx, db[i], h, c, 0) < 0) d[n++] = db[i];
  } else
    for (int64_t i = 0; i < na; i++)
      if ((tbl_search(db, nb, b->idx, da[i], h, c, 0) >= 0) == (op == 1)) d[n++] = da[i];
  Tbl *t = tbl_of(d, n, h, c, 0, kind);
  free(d);
  return t;
}

// A table of entries `es` (a dict's pairs, `t`'s keys in `t`'s order: its
// `map_values`), sharing `t`'s index.
Tbl *strata_tbl_with_entries(List *es, Tbl *t) {
  strata_dup(es);
  strata_dup(t->idx);
  return tbl_new(es, t->idx);
}

// Whether a table can be changed in place: it, its entries and its index
// (if any) held once, their buffers all their own.
_Bool strata_tbl_owned(Tbl *t) {
  List *es = t->es, *idx = t->idx;
  return strata_unique(t) && strata_unique(es) && strata_unique(es->buf) && es->off == 0 && es->len == es->buf->used &&
         (idx->len == 0 || (strata_unique(idx) && strata_unique(idx->buf) && idx->off == 0));
}

static void tbl_free_index(List *idx) {
  if (idx->len == 0) return;
  strata_free(idx->buf);
  strata_free(idx);
}

// In an owned table: `x` (held already), whose key it hasn't, after the
// entries.
void strata_tbl_add(Tbl *t, int64_t x, TblHash h, int64_t dict) {
  int64_t n = t->es->len;
  append(t->es, x);
  if (n + 1 <= TBL_SMALL) return;
  List *idx = t->idx;
  if (idx->len == 0 || 2 * (n + 1) > idx->len) {
    t->idx = tbl_index(tbl_data(t->es), n + 1, h, dict);
    tbl_free_index(idx);
  } else tbl_place(tbl_data(idx), idx->len, tbl_mix(h(tbl_key(x, dict))), n);
}

// In an owned table: entry `e` taken out, the last entry moved into its
// place (O(1): its slot emptied, the last entry's slot renumbered), and
// returned for the caller to drop.
int64_t strata_tbl_remove_at(Tbl *t, int64_t e, TblHash h, int64_t dict) {
  List *es = t->es;
  Buf *b = es->buf;
  int64_t last = es->len - 1, old = b->data[e];
  List *idx = t->idx;
  if (idx->len != 0) {
    if (last <= TBL_SMALL) {
      t->idx = NO_LIST;
      tbl_free_index(idx);
    } else {
      int64_t cap = idx->len, *s = tbl_data(idx);
      tbl_unplace(s, cap, tbl_slot_of(s, cap, tbl_mix(h(tbl_key(old, dict))), e), b->data, h, dict);
      if (e != last) {
        uint64_t j = tbl_slot_of(s, cap, tbl_mix(h(tbl_key(b->data[last], dict))), last);
        s[j] = (s[j] & ~(int64_t)0xFFFFFFFF) | (e + 1);
      }
    }
  }
  b->data[e] = b->data[last];
  b->used--;
  es->len--;
  return old;
}

// `a == b`: the same keys, and equal entries (`eq`, of entries) for each.
_Bool strata_tbl_eq(Tbl *a, Tbl *b, TblHash h, TblCmp c, TblCmp eq, int64_t dict) {
  if (a == b) return 1;
  int64_t n = a->es->len, *da = tbl_data(a->es), *db = tbl_data(b->es);
  if (n != b->es->len) return 0;
  for (int64_t i = 0; i < n; i++) {
    int64_t e = tbl_search(db, n, b->idx, tbl_key(da[i], dict), h, c, dict);
    if (e < 0 || !eq(da[i], db[e])) return 0;
  }
  return 1;
}

// `xs` (n of them) sorted by `c`, using `tmp` (merge sort).
static void tbl_sort(int64_t *xs, int64_t *tmp, int64_t n, TblCmp c) {
  if (n < 2) return;
  int64_t h = n / 2;
  tbl_sort(xs, tmp, h, c);
  tbl_sort(xs + h, tmp, n - h, c);
  int64_t i = 0, j = h, k = 0;
  while (i < h && j < n) tmp[k++] = c(xs[j], xs[i]) < 0 ? xs[j++] : xs[i++];
  while (i < h) tmp[k++] = xs[i++];
  while (j < n) tmp[k++] = xs[j++];
  memcpy(xs, tmp, (size_t)n * 8);
}

// The order of two tables: their entries sorted (`c`, of entries), then
// compared as lists are.
int64_t strata_tbl_cmp(Tbl *a, Tbl *b, TblCmp c) {
  int64_t na = a->es->len, nb = b->es->len;
  int64_t *x = malloc((size_t)(na + nb + 1) * 8), *tmp = malloc((size_t)(na > nb ? na : nb) * 8 + 8);
  int64_t *y = x + na;
  memcpy(x, tbl_data(a->es), (size_t)na * 8);
  memcpy(y, tbl_data(b->es), (size_t)nb * 8);
  tbl_sort(x, tmp, na, c);
  tbl_sort(y, tmp, nb, c);
  int64_t r = 0;
  for (int64_t i = 0; i < na && i < nb && r == 0; i++) r = c(x[i], y[i]);
  if (r == 0) r = na < nb ? -1 : na > nb;
  free(x);
  free(tmp);
  return r;
}

// A table's hash: its entries' (`eh`), in any order.
int64_t strata_tbl_hash(Tbl *t, TblHash eh) {
  uint64_t s = 0;
  int64_t *d = tbl_data(t->es);
  for (int64_t i = 0; i < t->es->len; i++) s += tbl_mix(eh(d[i]));
  return (int64_t)s;
}

// A list's hash: its elements' (`eh`), in order.
int64_t strata_list_hash(List *l, TblHash eh) {
  uint64_t s = (uint64_t)l->len;
  int64_t *d = tbl_data(l);
  for (int64_t i = 0; i < l->len; i++) s = (s * 1099511628211ull) ^ (uint64_t)eh(d[i]);
  return (int64_t)s;
}

// An `Int`'s hash: a small one is itself; big ones (outside [-2^62, 2^62))
// all hash the same.
int64_t strata_int_hash(int64_t v) {
  return v < -(INT64_C(1) << 62) || v > (INT64_C(1) << 62) - 1 ? 0x5bd1e995 : v;
}

// A string's hash, from its bytes, 8 at a time.
int64_t strata_str_hash(Str *s) {
  uint64_t h = 0x9E3779B97F4A7C15ull ^ (uint64_t)s->len;
  const char *p = s->bytes;
  int64_t n = s->len;
  for (; n >= 8; p += 8, n -= 8) {
    uint64_t w;
    memcpy(&w, p, 8);
    h = (h ^ w) * 0xbf58476d1ce4e5b9ull;
    h ^= h >> 31;
  }
  uint64_t w = 0;
  memcpy(&w, p, (size_t)n);
  h = (h ^ w) * 0x94d049bb133111ebull;
  h ^= h >> 29;
  return (int64_t)h;
}

#ifndef STRATA_CORE_RT
// The elements from `from` to `to` (already within 0..len): shares the buffer.
List *strata_list_slice(List *xs, int64_t from, int64_t to) {
  List *l = strata_alloc(sizeof(List));
  strata_dup(xs->buf);
  *l = (List){to - from, xs->buf, xs->off + from};
  return l;
}

List *strata_list_concat(List *a, List *b, int64_t counted) {
  if (b->len == 0) return strata_dup(a), a;
  if (a->len == 0) return strata_dup(b), b;
  Buf *n = buf_new(a->len + b->len);
  memcpy(n->data, a->buf->data + a->off, (size_t)a->len * 8);
  memcpy(n->data + a->len, b->buf->data + b->off, (size_t)b->len * 8);
  dup_slots(n->data, a->len + b->len, counted);
  n->used = a->len + b->len;
  List *l = strata_alloc(sizeof(List));
  *l = (List){a->len + b->len, n, 0};
  return l;
}

// `xs += ys` (core.list's `list_extend`; without core.rt, a new list each
// time).
List *strata_list_extend(List *a, List *b, int64_t counted) { return strata_list_concat(a, b, counted); }

// `s += t` (core.str's `str_append`; without core.rt, a new string each time).
Str *strata_str_append(Str *a, Str *b, _Bool mine) { return strata_concat(a, b); }

// `set_at` on a list something else holds too: a copy of `xs`, its other
// elements held once more, element `i` replaced by `x` (handed over).
List *strata_list_set(List *xs, int64_t i, int64_t x, int64_t counted) {
  Buf *n = buf_new(xs->len);
  memcpy(n->data, xs->buf->data + xs->off, (size_t)xs->len * 8);
  dup_slots(n->data, i, counted);
  dup_slots(n->data + i + 1, xs->len - i - 1, counted);
  n->data[i] = x;
  n->used = xs->len;
  List *l = strata_alloc(sizeof(List));
  *l = (List){xs->len, n, 0};
  return l;
}

List *strata_range(int64_t lo, int64_t hi) {
  int64_t n = hi > lo ? hi - lo : 0;
  List *l = strata_list_new(n);
  for (int64_t i = 0; i < n; i++) l->buf->data[i] = lo + i;
  l->buf->used = n;
  l->len = n;
  return l;
}
#endif

// ---- `--copies`: the copies of values a program makes, reported at its end ----
//
// Code compiled with `--copies` (`strata run`, `test` and `build`) calls
// `strata_copied` where a value about to be changed is copied because
// something else still holds it (and `strata_copies_push` /
// `strata_copies_append` before an extension, which copies when its buffer
// can't be extended in place). Each place's copies are added up (how many,
// their bytes, the largest), and listed on standard error when the program
// ends, the most bytes first. Code compiled without it calls none of these.

typedef struct {
  Str *site, *what, *why;
  int64_t kind, times, bytes, most, held;
} CopySite;

static CopySite *copy_sites;
static int64_t ncopy_sites, capcopy_sites;
static pthread_mutex_t copies_lock = PTHREAD_MUTEX_INITIALIZER;

static _Bool str_same(Str *a, Str *b) { return a == b || (a->len == b->len && !memcmp(a->bytes, b->bytes, (size_t)a->len)); }

// `n` with its thousands separated by commas.
static void copies_count(char *out, size_t size, int64_t n) {
  char d[32];
  int k = snprintf(d, sizeof d, "%lld", (long long)n), j = 0;
  for (int i = 0; i < k && j < (int)size - 2; i++) {
    if (i > 0 && (k - i) % 3 == 0) out[j++] = ',';
    out[j++] = d[i];
  }
  out[j] = 0;
}

static void copies_bytes(char *out, size_t size, int64_t n) {
  char c[40];
  if (n >= 10 * 1000 * 1000) copies_count(c, sizeof c, n / 1000000), snprintf(out, size, "%s MB", c);
  else if (n >= 10 * 1000) copies_count(c, sizeof c, n / 1000), snprintf(out, size, "%s kB", c);
  else copies_count(c, sizeof c, n), snprintf(out, size, "%s bytes", c);
}

static int copies_order(const void *a, const void *b) {
  const CopySite *x = a, *y = b;
  return x->bytes < y->bytes ? 1 : x->bytes > y->bytes ? -1 : 0;
}

static void copies_report(void) {
  fflush(stdout);
  pthread_mutex_lock(&copies_lock);
  if (ncopy_sites == 0) {
    fprintf(stderr, "copies: none\n");
    pthread_mutex_unlock(&copies_lock);
    return;
  }
  qsort(copy_sites, (size_t)ncopy_sites, sizeof(CopySite), copies_order);
  int64_t times = 0, bytes = 0;
  for (int64_t i = 0; i < ncopy_sites; i++) times += copy_sites[i].times, bytes += copy_sites[i].bytes;
  char t[40], b[40], m[40];
  copies_count(t, sizeof t, times);
  copies_bytes(b, sizeof b, bytes);
  fprintf(stderr, "copies: %s cop%s, %s, at %lld place%s (the most bytes first):\n", t, times == 1 ? "y" : "ies", b,
          (long long)ncopy_sites, ncopy_sites == 1 ? "" : "s");
  for (int64_t i = 0; i < ncopy_sites; i++) {
    CopySite *c = &copy_sites[i];
    copies_count(m, sizeof m, c->most);
    copies_count(t, sizeof t, c->times);
    copies_bytes(b, sizeof b, c->bytes);
    const char *unit = c->kind == 3 ? "byte" : c->kind == 4 ? "field" : c->kind == 0 ? "item" : "entr";
    const char *plural = c->most == 1 ? (c->kind == 1 ? "y" : "") : (c->kind == 1 ? "ies" : "s");
    fprintf(stderr, "  copied a %.*s of %s%s %s%s at %.*s: %s time%s, %s (", (int)c->what->len, c->what->bytes,
            c->times > 1 && c->most > 1 ? "up to " : "", m, unit, plural, (int)c->site->len, c->site->bytes, t,
            c->times == 1 ? "" : "s", b);
    if (c->why->len) fprintf(stderr, "%.*s", (int)c->why->len, c->why->bytes);
    else if (c->held > 1) fprintf(stderr, "shared: held %lld times", (long long)c->held);
    else fprintf(stderr, "held once, but what holds it is shared");
    fprintf(stderr, ")\n");
  }
  pthread_mutex_unlock(&copies_lock);
}

// How many hold object `p` (its count word's count bits; 0 for a constant).
static int64_t copies_held(void *p) { return __atomic_load_n((int64_t *)p - 1, __ATOMIC_ACQUIRE) & ((INT64_C(1) << 40) - 1); }

static void copies_note(int64_t kind, int64_t items, int64_t bytes, int64_t held, Str *what, Str *site, Str *why) {
  pthread_mutex_lock(&copies_lock);
  if (capcopy_sites == 0) atexit(copies_report);
  CopySite *c = NULL;
  for (int64_t i = 0; i < ncopy_sites && !c; i++)
    if (copy_sites[i].kind == kind && str_same(copy_sites[i].site, site) && str_same(copy_sites[i].what, what) &&
        str_same(copy_sites[i].why, why))
      c = &copy_sites[i];
  if (!c) {
    if (ncopy_sites == capcopy_sites) {
      capcopy_sites = capcopy_sites ? 2 * capcopy_sites : 16;
      copy_sites = realloc(copy_sites, (size_t)capcopy_sites * sizeof(CopySite));
    }
    c = &copy_sites[ncopy_sites++];
    *c = (CopySite){site, what, why, kind, 0, 0, 0, held};
  }
  c->times++;
  c->bytes += bytes;
  if (items > c->most) c->most = items;
  if (held > c->held) c->held = held;
  pthread_mutex_unlock(&copies_lock);
}

static Str *copies_text(Str **cell, const char *c) {
  if (!*cell) *cell = str_c(c);
  return *cell;
}

// A program compiled with `--copies` starts: "copies: none" is reported at
// its end if it copies nothing.
void strata_copies_start(void) {
  pthread_mutex_lock(&copies_lock);
  if (capcopy_sites == 0) {
    atexit(copies_report);
    capcopy_sites = 16;
    copy_sites = malloc((size_t)capcopy_sites * sizeof(CopySite));
  }
  pthread_mutex_unlock(&copies_lock);
}

// A copy of `obj` (`kind`: 0 a list, 1 a dict or set, 3 a string, 4 a record
// or tuple of `n` fields), a `what` (its type), at `site` (a location), `why`
// ("": its count says), unless it is empty or a constant.
void strata_copied(int64_t kind, void *obj, int64_t n, Str *what, Str *site, Str *why) {
  int64_t items, bytes;
  if (kind == 0) {
    items = ((List *)obj)->len;
    bytes = 24 + 8 * items;
  } else if (kind == 1) {
    Tbl *t = obj;
    items = t->es->len;
    bytes = 24 + 24 + 8 * items + (t->idx->len ? 24 + 8 * t->idx->len : 0);
  } else if (kind == 3) {
    items = ((Str *)obj)->len;
    bytes = 8 + items;
  } else {
    items = n;
    bytes = 8 * (n + 1);
  }
  int64_t held = copies_held(obj);
  // (a constant, `Dict { 1: 2 }`, is made by its first change: not a copy)
  if ((items == 0 && kind != 4) || held == 0) return;
  static Str *iterated;
  if (kind == 1 && held == 1 && why->len == 0 && copies_held(((Tbl *)obj)->es) > 1)
    why = copies_text(&iterated, "its entries are held elsewhere too: a `for` over it, or a list taken from it");
  copies_note(kind, items, bytes, held, what, site, why);
}

// Before `push` (`n` 1) or `xs += ys` (`n` the length of `ys`): a copy when
// a slot of `xs`'s buffer it needs is taken already (another list made from
// the same one grew past it, or `xs` is a slice); running out of room only
// grows the buffer (doubling, not counted).
void strata_copies_push(List *xs, int64_t n, Str *what, Str *site) {
  static Str *why;
  if (xs->len == 0 || n == 0 || xs->off + xs->len == xs->buf->used) return;
  copies_note(0, xs->len, 24 + 8 * xs->len, copies_held(xs->buf), what, site,
              copies_text(&why, "its buffer is used past its end: another list made from it grew, or it is a slice"));
}

// Before `s += t` (`mine`: nothing but the variable holds what holds `s`):
// a copy when `s` can't be extended where it is because something else
// holds it.
void strata_copies_append(Str *s, _Bool mine, Str *site) {
  static Str *what, *none, *outer;
  if (s->len == 0 || (mine && copies_held(s) == 1)) return;
  copies_note(3, s->len, 8 + s->len, copies_held(s), copies_text(&what, "String"), site,
              mine ? copies_text(&none, "") : copies_text(&outer, "held once, but what holds it is shared"));
}

// Whether to color what goes to stream `fd`: a terminal, unless `NO_COLOR`
// is set or `TERM` is `dumb`; `FORCE_COLOR` forces it (as std.term decides).
_Bool strata_host_terminal(int64_t fd);
static _Bool colors(int fd) {
  const char *f = getenv("FORCE_COLOR");
  if (f && *f) return 1;
  const char *n = getenv("NO_COLOR");
  if (n && *n) return 0;
  const char *t = getenv("TERM");
  if (t && !strcmp(t, "dumb")) return 0;
  return strata_host_terminal(fd);
}

// `main` returned `Err(e)` (`error:` in red on a terminal).
void strata_main_error(Str *shown) {
  fflush(stdout);
  if (colors(2)) fprintf(stderr, "\x1b[1;31merror:\x1b[0m %.*s\n", (int)shown->len, shown->bytes);
  else fprintf(stderr, "error: %.*s\n", (int)shown->len, shown->bytes);
}

// ---- output ----

void strata_print(Str *s) {
  if (RS->capturing) {
    keep_printed(s->bytes, (size_t)s->len);
    keep_printed("\n", 1);
    return;
  }
  flockfile(stdout);
  fwrite(s->bytes, 1, (size_t)s->len, stdout);
  fputc('\n', stdout);
  funlockfile(stdout);
}

// ---- the system (the `host_*` operations of std) ----
//
// As the interpreter's (`Interp::host_call`): a failure is a kind and a
// message, the message as Rust writes an OS error ("No such file or
// directory (os error 2)"). The generated code wraps results in `Option`
// and `Result` (whose tags only it knows).

static int prog_argc;
// The program's name, as `--help` shows it: its file's, without the folder.
static char *prog_name = "main";
static char **prog_argv;

// Running out of stack (deep recursion) is a fault, as in the interpreter:
// a guard page hit near the end of the thread's stack, caught on a stack of
// its own, is `strata_fault` (so a test's trap catches it too). Any other
// bad access is reported as a crash.
static _Thread_local char *stack_lo, *stack_hi;

static void on_segv(int sig, siginfo_t *info, void *ctx) {
  (void)ctx;
  char *at = info->si_addr;
  if (stack_hi && at < stack_hi && at >= stack_lo - 65536) strata_fault(6, str_c("stack overflow (recursion too deep)"));
  // a crash in a fuzz case (`verify` running a function in the compiler's
  // own process): the case ends, not the compiler, which reads "crash" as
  // a case it could not run
  if (RS->trap && RS->counting) {
    RS->counting = 0;
    strata_fault(4, str_c(sig == SIGBUS ? "crash (signal SIGBUS)" : "crash (signal SIGSEGV)"));
  }
  // a crash: what was running, then the signal as it would have been
  fflush(stdout);
  fprintf(stderr, "crash (signal %d) in:\n", sig);
  print_stack();
  signal(sig, SIG_DFL);
  raise(sig);
}

// Once per thread: the thread's stack (the JIT runs on strata's own, larger
// than the process's limit) and a stack for the handler.
static _Thread_local char *alt;

static void free_alt_stack(void) {
  stack_t off = {.ss_flags = SS_DISABLE};
  sigaltstack(&off, NULL);
  free(alt);
  alt = NULL;
}

static void catch_overflow(void) {
  if (stack_hi) return;
  if (!alt) alt = malloc(65536);
  stack_t ss = {.ss_sp = alt, .ss_size = 65536, .ss_flags = 0};
  sigaltstack(&ss, NULL);
  struct sigaction sa;
  memset(&sa, 0, sizeof sa);
  sa.sa_sigaction = on_segv;
  sa.sa_flags = SA_SIGINFO | SA_ONSTACK | SA_NODEFER;
  sigaction(SIGSEGV, &sa, NULL);
  sigaction(SIGBUS, &sa, NULL);
#ifdef __APPLE__
  stack_hi = pthread_get_stackaddr_np(pthread_self());
  stack_lo = stack_hi - pthread_get_stacksize_np(pthread_self());
#else
  pthread_attr_t a;
  void *addr;
  size_t size;
  if (pthread_getattr_np(pthread_self(), &a) == 0) {
    pthread_attr_getstack(&a, &addr, &size);
    pthread_attr_destroy(&a);
    stack_lo = addr;
    stack_hi = (char *)addr + size;
  }
#endif
}

// The program's arguments, without its name; a closed pipe ends it quietly.
void strata_init(int32_t argc, char **argv) {
  CALLS.runs = 0;
  RS->rng = 7;
  catch_overflow();
  prog_argc = argc > 0 ? argc - 1 : 0;
  prog_argv = argv + 1;
  if (argc > 0 && argv[0]) prog_name = argv[0];
  signal(SIGPIPE, SIG_IGN);
  // (what compile-time code made before, in this process, isn't the program's)
  static _Bool registered;
  const char *m = getenv("STRATA_MEMORY");
  checking = m && !strcmp(m, "check");
#ifdef STRATA_CORE_RT
  strata_memory_start(checking);
#else
  allocated = freed = 0;
  nmade = 0;
#endif
  set_threaded(THREADED);
  if (m && !registered) registered = 1, atexit(memory_stats);
}

// A library's start (`strata build --lib`), when it is loaded, before the
// host calls what it exports: the runtime set up as for a program, but the
// host keeps its signals, and there are no arguments.
void strata_lib_init(void) {
  CALLS.runs = 0;
  RS->rng = 7;
  const char *m = getenv("STRATA_MEMORY");
  checking = m && !strcmp(m, "check");
#ifdef STRATA_CORE_RT
  strata_memory_start(checking);
#else
  allocated = freed = 0;
  nmade = 0;
#endif
  set_threaded(THREADED);
}

Str *strata_prog_name(void) {
  const char *slash = strrchr(prog_name, '/');
  return str_c(slash ? slash + 1 : prog_name);
}

// The options could not be read: the help asked for (on standard output), or
// what is wrong with the arguments (on standard error).

// `--help` (its section headings, `Usage:`, `Options:`…, in bold on a
// terminal), or what is wrong with the arguments (`error:` in red).
void strata_options_error(_Bool help, Str *text) {
  if (help) {
    _Bool on = colors(1);
    const char *s = text->bytes, *end = s + text->len;
    while (s < end) {
      const char *nl = memchr(s, '\n', (size_t)(end - s));
      const char *e = nl ? nl : end;
      // a heading: a capitalized word or two at the start, then `:`
      const char *c = memchr(s, ':', (size_t)(e - s));
      _Bool heading = on && c && s[0] >= 'A' && s[0] <= 'Z' && (c + 1 == e || c[1] == ' ') && c - s < 20 && !memchr(s, '`', (size_t)(c - s));
      if (heading) {
        fputs("\x1b[1m", stdout);
        fwrite(s, 1, (size_t)(c + 1 - s), stdout);
        fputs("\x1b[0m", stdout);
        fwrite(c + 1, 1, (size_t)(e - c - 1), stdout);
      } else {
        fwrite(s, 1, (size_t)(e - s), stdout);
      }
      if (nl) fputc('\n', stdout);
      s = nl ? nl + 1 : end;
    }
    fflush(stdout);
  } else {
    fflush(stdout);
    if (colors(2)) fprintf(stderr, "\x1b[1;31merror:\x1b[0m %.*s\n", (int)text->len, text->bytes);
    else fprintf(stderr, "error: %.*s\n", (int)text->len, text->bytes);
  }
}

// A C copy of `s`, for the OS: valid until this thread's next call (one
// buffer per thread, reused: tasks run on threads of their own, and a buffer
// shared by them let one task's path replace another's between `cstr` and the
// call, so that a `remove` removed another task's file).
static char *cstr(Str *s) {
  static _Thread_local char *c;
  static _Thread_local size_t cap;
  if ((size_t)s->len + 1 > cap) c = realloc(c, cap = (size_t)s->len + 1);
  memcpy(c, s->bytes, (size_t)s->len);
  c[s->len] = 0;
  return c;
}

// The failure in `errno`, as (kind, message).
static void os_error(Str **kind, Str **msg) {
  int e = errno;
  const char *k = "other";
  if (e == ENOENT) k = "not_found";
  else if (e == EACCES || e == EPERM) k = "permission_denied";
  else if (e == EEXIST) k = "already_exists";
  else if (e == ENOTDIR) k = "not_a_directory";
  else if (e == EISDIR) k = "is_a_directory";
  char buf[300], text[200];
  if (strerror_r(e, text, sizeof text) != 0) snprintf(text, sizeof text, "error %d", e);
  snprintf(buf, sizeof buf, "%s (os error %d)", text, e);
  *kind = str_c(k);
  *msg = str_c(buf);
}

void strata_host_write(int64_t fd, Str *text) {
  // in a test, the terminal's output is the test's own, as `print`'s is
  if (RS->capturing) {
    keep_printed(text->bytes, (size_t)text->len);
    return;
  }
  FILE *f = fd == 2 ? stderr : stdout;
  if (fd == 2) fflush(stdout);
  if (fwrite(text->bytes, 1, (size_t)text->len, f) < (size_t)text->len || fflush(f) != 0) {
    if (errno == EPIPE) exit(0);
  }
}

// Whether `f` has input read from its descriptor and not taken yet (then a
// `getc` doesn't wait). Platforms whose `FILE` we can't look into say no:
// a line already buffered there waits for more input (or the end) first.
static _Bool input_buffered(FILE *f) {
#if defined(__APPLE__) || defined(__FreeBSD__) || defined(__OpenBSD__) || defined(__NetBSD__)
  return f->_r > 0;
#elif defined(__GLIBC__)
  return f->_IO_read_ptr < f->_IO_read_end;
#else
  (void)f;
  return 0;
#endif
}

// A line without its end, or NULL at the end of the input. In a task (or a
// test) it can be cancelled while it waits: NULL then too, and the compiled
// code leaves (`cancel_point`); what it had read of the line is dropped.
Str *strata_host_read_line(void) {
  if (!self_task && !contained) {
    size_t cap = 0;
    char *line = NULL;
    ssize_t n = getline(&line, &cap, stdin);
    if (n <= 0) {
      free(line);
      return NULL;
    }
    if (n > 0 && line[n - 1] == '\n') n--;
    if (n > 0 && line[n - 1] == '\r') n--;
    Str *s = str_from(line, n);
    free(line);
    return s;
  }
  // a character at a time from stdin's buffer; before each refill, a
  // cancellable wait for the descriptor (the refill then takes what is there)
  size_t cap = 128, n = 0;
  char *line = malloc(cap);
  int c = EOF;
  // (stdin isn't locked across the wait: another task reading it would be
  // stuck on the lock, where it can't be cancelled)
  for (;;) {
    if (!input_buffered(stdin) && !wait_readable(fileno(stdin))) {
      free(line);
      return NULL;
    }
    c = getc(stdin);
    if (c == EOF) break;
    if (n + 1 >= cap) line = realloc(line, cap *= 2);
    line[n++] = (char)c;
    if (c == '\n') break;
  }
  if (n == 0 && c == EOF) {
    free(line);
    return NULL;
  }
  if (n > 0 && line[n - 1] == '\n') n--;
  if (n > 0 && line[n - 1] == '\r') n--;
  Str *s = str_from(line, (int64_t)n);
  free(line);
  return s;
}

Str *strata_host_read_file(Str *path, Str **kind, Str **msg) {
  FILE *f = fopen(cstr(path), "rb");
  struct stat st;
  if (!f) {
    os_error(kind, msg);
    return NULL;
  }
  if (fstat(fileno(f), &st) == 0 && S_ISDIR(st.st_mode)) {
    errno = EISDIR;
    os_error(kind, msg);
    fclose(f);
    return NULL;
  }
  size_t cap = 4096, len = 0;
  char *buf = malloc(cap);
  size_t got;
  while ((got = fread(buf + len, 1, cap - len, f)) > 0) {
    len += got;
    if (len == cap) buf = realloc(buf, cap *= 2);
  }
  fclose(f);
  Str *s = str_from(buf, (int64_t)len);
  free(buf);
  return s;
}

// The file's bytes, each a small `Int`.
List *strata_host_read_bytes(Str *path, Str **kind, Str **msg) {
  Str *s = strata_host_read_file(path, kind, msg);
  if (!s) return NULL;
  List *l = strata_list_new(s->len);
  for (int64_t i = 0; i < s->len; i++) l = append(l, (int64_t)(unsigned char)s->bytes[i]);
  if (strata_release(s)) strata_free(s);
  return l;
}

_Bool strata_host_write_file(Str *path, Str *text, _Bool append, Str **kind, Str **msg) {
  FILE *f = fopen(cstr(path), append ? "ab" : "wb");
  if (!f) {
    os_error(kind, msg);
    return 0;
  }
  if (fwrite(text->bytes, 1, (size_t)text->len, f) < (size_t)text->len) {
    os_error(kind, msg);
    fclose(f);
    return 0;
  }
  fclose(f);
  return 1;
}

_Bool strata_host_exists(Str *path) {
  struct stat st;
  return stat(cstr(path), &st) == 0;
}

static int by_name(const void *a, const void *b) {
  Str *x = *(Str *const *)a, *y = *(Str *const *)b;
  return (int)strata_str_cmp(x, y);
}

// The names in a directory, sorted.
List *strata_host_list_dir(Str *path, Str **kind, Str **msg) {
  DIR *d = opendir(cstr(path));
  if (!d) {
    os_error(kind, msg);
    return NULL;
  }
  List *l = strata_list_new(16);
  struct dirent *e;
  while ((e = readdir(d))) {
    if (!strcmp(e->d_name, ".") || !strcmp(e->d_name, "..")) continue;
    l = append(l, (int64_t)(intptr_t)str_c(e->d_name));
  }
  closedir(d);
  qsort(l->buf->data + l->off, (size_t)l->len, 8, by_name);
  return l;
}

// `mkdir -p`.
_Bool strata_host_make_dir(Str *path, Str **kind, Str **msg) {
  char *p = cstr(path);
  for (char *c = p + 1; *c; c++) {
    if (*c != '/') continue;
    *c = 0;
    if (mkdir(p, 0777) != 0 && errno != EEXIST) {
      os_error(kind, msg);
      return 0;
    }
    *c = '/';
  }
  struct stat st;
  if (mkdir(p, 0777) != 0 && !(errno == EEXIST && stat(p, &st) == 0 && S_ISDIR(st.st_mode))) {
    os_error(kind, msg);
    return 0;
  }
  return 1;
}

_Bool strata_host_remove(Str *path, Str **kind, Str **msg) {
  char *p = cstr(path);
  struct stat st;
  int r = stat(p, &st) == 0 && S_ISDIR(st.st_mode) ? rmdir(p) : unlink(p);
  if (r != 0) {
    os_error(kind, msg);
    return 0;
  }
  return 1;
}

// Whether `path` is a folder (following links).
_Bool strata_host_is_dir(Str *path) {
  struct stat st;
  return stat(cstr(path), &st) == 0 && S_ISDIR(st.st_mode);
}

// Replace the file's contents with `bytes` (each an `Int` from 0 to 255).
_Bool strata_host_write_bytes(Str *path, List *bytes, Str **kind, Str **msg) {
  FILE *f = fopen(cstr(path), "wb");
  if (!f) {
    os_error(kind, msg);
    return 0;
  }
  size_t n = (size_t)bytes->len;
  unsigned char *buf = malloc(n ? n : 1);
  for (size_t i = 0; i < n; i++) buf[i] = (unsigned char)bytes->buf->data[bytes->off + (int64_t)i];
  size_t put = fwrite(buf, 1, n, f);
  free(buf);
  if (put < n) {
    os_error(kind, msg);
    fclose(f);
    return 0;
  }
  if (fclose(f) != 0) {
    os_error(kind, msg);
    return 0;
  }
  return 1;
}

// Move a file or folder (replacing a file at `to`).
_Bool strata_host_rename(Str *from, Str *to, Str **kind, Str **msg) {
  char *a = strndup(from->bytes, (size_t)from->len);
  int r = rename(a, cstr(to));
  free(a);
  if (r != 0) {
    os_error(kind, msg);
    return 0;
  }
  return 1;
}

static int remove_tree(const char *p) {
  struct stat st;
  if (lstat(p, &st) != 0) return -1;
  if (!S_ISDIR(st.st_mode)) return unlink(p);
  DIR *d = opendir(p);
  if (!d) return -1;
  struct dirent *e;
  int r = 0;
  while (r == 0 && (e = readdir(d))) {
    if (!strcmp(e->d_name, ".") || !strcmp(e->d_name, "..")) continue;
    size_t n = strlen(p) + strlen(e->d_name) + 2;
    char *c = malloc(n);
    snprintf(c, n, "%s/%s", p, e->d_name);
    r = remove_tree(c);
    free(c);
  }
  closedir(d);
  return r == 0 ? rmdir(p) : r;
}

// Remove a file, or a folder and all it holds (`rm -r`; links are removed,
// not followed).
_Bool strata_host_remove_all(Str *path, Str **kind, Str **msg) {
  char *p = strndup(path->bytes, (size_t)path->len);
  int r = remove_tree(p);
  free(p);
  if (r != 0) {
    os_error(kind, msg);
    return 0;
  }
  return 1;
}

// The absolute path, links and `..` resolved; the file must exist.
Str *strata_host_absolute(Str *path, Str **kind, Str **msg) {
  char *r = realpath(cstr(path), NULL);
  if (!r) {
    os_error(kind, msg);
    return NULL;
  }
  Str *s = str_c(r);
  free(r);
  return s;
}

// The current folder, absolute (`""` if it was removed).
Str *strata_host_current_dir(void) {
  char *r = getcwd(NULL, 0);
  Str *s = str_c(r ? r : "");
  free(r);
  return s;
}

// What is left of standard input, whole (after the lines `read_line` took).
Str *strata_host_read_stdin(void) {
  size_t cap = 4096, len = 0;
  char *buf = malloc(cap);
  size_t got;
  while ((got = fread(buf + len, 1, cap - len, stdin)) > 0) {
    len += got;
    if (len == cap) buf = realloc(buf, cap *= 2);
  }
  clearerr(stdin);
  Str *s = str_from(buf, (int64_t)len);
  free(buf);
  return s;
}

// Whether standard output goes to a pager that shows it on a terminal
// (`strata`'s, compiler/c/driver.c): it counts as one.
static _Bool stdout_paged = 0;
void strata_host_paged(void) { stdout_paged = 1; }

// Whether a stream (0: standard input, 1: output, 2: error) is a terminal
// (not a pipe or a file), or a pager on one.
_Bool strata_host_terminal(int64_t fd) { return (fd == 1 && stdout_paged) || (fd >= 0 && fd <= 2 && isatty((int)fd)); }

List *strata_host_args(void) {
  List *l = strata_list_new(prog_argc);
  for (int i = 0; i < prog_argc; i++) l = append(l, (int64_t)(intptr_t)str_c(prog_argv[i]));
  return l;
}

Str *strata_host_env_var(Str *name) {
  const char *v = getenv(cstr(name));
  return v ? str_c(v) : NULL;
}

int64_t strata_pid(void) { return (int64_t)getpid(); }

// Runs a program (looked up in PATH, never through a shell) with its
// arguments in a working directory (`""`: this process's), its standard input
// empty, and waits: `[exit code, stdout, stderr]` (the code as decimal text; a
// program killed by a signal gives 128 + the signal, as shells do), or NULL and
// the failure when it can't be started.
static Str *read_all_into(char **b, size_t *n, size_t *cap, int fd) {
  char tmp[65536];
  ssize_t r = read(fd, tmp, sizeof tmp);
  if (r <= 0) return NULL;
  if (*n + (size_t)r > *cap) *b = realloc(*b, *cap = (*n + (size_t)r) * 2);
  memcpy(*b + *n, tmp, (size_t)r);
  *n += (size_t)r;
  return (Str *)1;
}

// The environment for a child: this process's, with each `NAME=VALUE` of
// `env` set (replacing the variable of that name).
static char **child_environ(List *env) {
  extern char **environ;
  size_t n = 0;
  while (environ[n]) n++;
  char **out = calloc(n + (size_t)env->len + 1, sizeof(char *));
  size_t k = 0;
  for (size_t i = 0; i < n; i++) {
    const char *eq = strchr(environ[i], '=');
    size_t name = eq ? (size_t)(eq - environ[i]) : strlen(environ[i]);
    _Bool replaced = 0;
    for (int64_t j = 0; j < env->len && !replaced; j++) {
      Str *v = (Str *)(intptr_t)env->buf->data[env->off + j];
      replaced = (size_t)v->len > name && v->bytes[name] == '=' && !memcmp(v->bytes, environ[i], name);
    }
    if (!replaced) out[k++] = strdup(environ[i]);
  }
  for (int64_t j = 0; j < env->len; j++) {
    Str *v = (Str *)(intptr_t)env->buf->data[env->off + j];
    out[k++] = strndup(v->bytes, (size_t)v->len);
  }
  out[k] = NULL;
  return out;
}

// A pipe whose ends are closed in programs this process starts (FD_CLOEXEC);
// the runs below also start them with POSIX_SPAWN_CLOEXEC_DEFAULT, which
// covers the moment between `pipe` and `fcntl`.
static int cloexec_pipe(int p[2]) {
  if (pipe(p) != 0) return -1;
  fcntl(p[0], F_SETFD, FD_CLOEXEC);
  fcntl(p[1], F_SETFD, FD_CLOEXEC);
  return 0;
}

// `input` is written to its standard input (then closed); `env` holds
// `NAME=VALUE`s set for it; `inherit`: it uses this process's standard input,
// output and error (a terminal, for an editor), and `[code, "", ""]` comes back.
List *strata_host_run(Str *program, List *args, Str *dir, Str *input, List *env, _Bool inherit, Str **kind, Str **msg) {
  int64_t argc = args->len;
  char **argv = calloc((size_t)argc + 2, sizeof(char *));
  argv[0] = strndup(program->bytes, (size_t)program->len);
  for (int64_t i = 0; i < argc; i++) {
    Str *a = (Str *)(intptr_t)args->buf->data[args->off + i];
    argv[i + 1] = strndup(a->bytes, (size_t)a->len);
  }
  int out[2] = {-1, -1}, err[2] = {-1, -1}, in[2] = {-1, -1};
  _Bool feed = !inherit && input->len > 0;
  if (!inherit) {
    if (cloexec_pipe(out) != 0) { os_error(kind, msg); return NULL; }
    if (cloexec_pipe(err) != 0) { os_error(kind, msg); close(out[0]); close(out[1]); return NULL; }
    if (feed && cloexec_pipe(in) != 0) { os_error(kind, msg); close(out[0]); close(out[1]); close(err[0]); close(err[1]); return NULL; }
  }
  // The child gets only the descriptors set up here (0, 1 and 2), none of
  // this process's others: tasks run programs at the same time, and a pipe of
  // one task's run left open in another's child kept its program's standard
  // input from ending (and its output, so the run never finished).
  posix_spawnattr_t at;
  posix_spawnattr_init(&at);
  posix_spawnattr_setflags(&at, POSIX_SPAWN_CLOEXEC_DEFAULT);
  posix_spawn_file_actions_t fa;
  posix_spawn_file_actions_init(&fa);
  if (inherit) {
    for (int k = 0; k <= 2; k++) posix_spawn_file_actions_addinherit_np(&fa, k);
  } else {
    if (feed) {
      posix_spawn_file_actions_adddup2(&fa, in[0], 0);
      posix_spawn_file_actions_addclose(&fa, in[0]);
      posix_spawn_file_actions_addclose(&fa, in[1]);
    } else {
      posix_spawn_file_actions_addopen(&fa, 0, "/dev/null", O_RDONLY, 0);
    }
    posix_spawn_file_actions_adddup2(&fa, out[1], 1);
    posix_spawn_file_actions_adddup2(&fa, err[1], 2);
    posix_spawn_file_actions_addclose(&fa, out[0]);
    posix_spawn_file_actions_addclose(&fa, err[0]);
    posix_spawn_file_actions_addclose(&fa, out[1]);
    posix_spawn_file_actions_addclose(&fa, err[1]);
  }
  if (inherit) {
    fflush(stdout);
    fflush(stderr);
  }
  char *d = dir->len ? strndup(dir->bytes, (size_t)dir->len) : NULL;
  if (d) posix_spawn_file_actions_addchdir_np(&fa, d);
  char **envp = child_environ(env);
  pid_t pid;
  int e = posix_spawnp(&pid, argv[0], &fa, &at, argv, envp);
  posix_spawn_file_actions_destroy(&fa);
  posix_spawnattr_destroy(&at);
  for (char **v = envp; *v; v++) free(*v);
  free(envp);
  if (!inherit) {
    close(out[1]);
    close(err[1]);
  }
  if (feed) close(in[0]);
  for (int64_t i = 0; i <= argc; i++) free(argv[i]);
  free(argv);
  if (e != 0) {
    if (!inherit) {
      close(out[0]);
      close(err[0]);
    }
    if (feed) close(in[1]);
    errno = e;
    os_error(kind, msg);
    char buf[600];
    snprintf(buf, sizeof buf, "can't run `%.*s`%s%s: %.*s", (int)program->len, program->bytes,
             d ? " in " : "", d ? d : "", (int)(*msg)->len, (*msg)->bytes);
    free(d);
    // (the OS's message, now inside the fuller one)
    str_drop(*msg);
    *msg = str_c(buf);
    return NULL;
  }
  free(d);
  char *ob = NULL, *eb = NULL;
  size_t on = 0, oc = 0, en = 0, ec = 0;
  if (!inherit) {
    // the input is written as the child reads it, while its output is taken
    // (a child that writes before it reads all its input must not block us)
    size_t sent = 0;
    // (a child that exits without reading breaks its pipe: the write fails
    // with EPIPE, no SIGPIPE; set on the pipe, not by `signal`, which is the
    // whole process's and raced between tasks)
    if (feed) {
      fcntl(in[1], F_SETFL, fcntl(in[1], F_GETFL) | O_NONBLOCK);
      fcntl(in[1], F_SETNOSIGPIPE, 1);
    }
    struct pollfd fds[3] = {{out[0], POLLIN, 0}, {err[0], POLLIN, 0}, {feed ? in[1] : -1, POLLOUT, 0}};
    int open_fds = 2;
    while (open_fds > 0 || fds[2].fd >= 0) {
      if (poll(fds, 3, -1) < 0) {
        if (errno == EINTR) continue;
        break;
      }
      for (int k = 0; k < 2; k++) {
        if (fds[k].fd < 0 || !(fds[k].revents & (POLLIN | POLLHUP | POLLERR))) continue;
        if (!(k == 0 ? read_all_into(&ob, &on, &oc, fds[k].fd) : read_all_into(&eb, &en, &ec, fds[k].fd))) {
          close(fds[k].fd);
          fds[k].fd = -1;
          open_fds--;
        }
      }
      if (fds[2].fd >= 0 && fds[2].revents) {
        ssize_t w = (fds[2].revents & POLLOUT) ? write(fds[2].fd, input->bytes + sent, (size_t)input->len - sent) : -1;
        if (w > 0) sent += (size_t)w;
        if (w < 0 && errno == EAGAIN) continue;
        if (w < 0 || sent == (size_t)input->len) {
          close(fds[2].fd);
          fds[2].fd = -1;
        }
      }
    }
  }
  int status = 0;
  while (waitpid(pid, &status, 0) < 0 && errno == EINTR) {}
  int code = WIFEXITED(status) ? WEXITSTATUS(status) : WIFSIGNALED(status) ? 128 + WTERMSIG(status) : -1;
  char cbuf[24];
  snprintf(cbuf, sizeof cbuf, "%d", code);
  List *l = strata_list_new(3);
  l = append(l, (int64_t)(intptr_t)str_c(cbuf));
  l = append(l, (int64_t)(intptr_t)str_from(ob ? ob : "", (int64_t)on));
  l = append(l, (int64_t)(intptr_t)str_from(eb ? eb : "", (int64_t)en));
  free(ob);
  free(eb);
  return l;
}

// ---- strings ----
//
// The string built-ins (`interp::Interp::builtin`). Strings are UTF-8 and
// indexed by character (code point). What depends on Unicode's tables (case
// mappings, `is_alpha`) takes a table the compiler made from Rust's own
// (`llvm.rs`, `unicode_table`), so both agree.
//
// Written in Strata in core.rt (core/rt.strata), which replaces these when it
// is linked in (`STRATA_CORE_RT`); they stay for the first stage of strata's
// build.

#ifndef STRATA_CORE_RT
// A character in UTF-8 at `o`: how many bytes it took.
static int utf8_put(int32_t c, char *o) {
  if (c < 0x80) return o[0] = (char)c, 1;
  if (c < 0x800) return o[0] = (char)(0xC0 | (c >> 6)), o[1] = (char)(0x80 | (c & 0x3F)), 2;
  if (c < 0x10000) return o[0] = (char)(0xE0 | (c >> 12)), o[1] = (char)(0x80 | ((c >> 6) & 0x3F)), o[2] = (char)(0x80 | (c & 0x3F)), 3;
  o[0] = (char)(0xF0 | (c >> 18)), o[1] = (char)(0x80 | ((c >> 12) & 0x3F)), o[2] = (char)(0x80 | ((c >> 6) & 0x3F)), o[3] = (char)(0x80 | (c & 0x3F));
  return 4;
}
#endif

#ifndef STRATA_CORE_RT
// The character at byte `i` (valid UTF-8), and its length in bytes.
static int32_t utf8_at(Str *s, int64_t i, int *n) {
  unsigned char c = (unsigned char)s->bytes[i];
  if (c < 0x80) return *n = 1, c;
  if (c < 0xE0) return *n = 2, ((c & 0x1F) << 6) | (s->bytes[i + 1] & 0x3F);
  if (c < 0xF0) return *n = 3, ((c & 0x0F) << 12) | ((s->bytes[i + 1] & 0x3F) << 6) | (s->bytes[i + 2] & 0x3F);
  return *n = 4, ((c & 0x07) << 18) | ((s->bytes[i + 1] & 0x3F) << 12) | ((s->bytes[i + 2] & 0x3F) << 6) | (s->bytes[i + 3] & 0x3F);
}

// The byte where character `k` starts (the end, past the last one).
static int64_t char_offset(Str *s, int64_t k) {
  int64_t i = 0;
  while (k > 0 && i < s->len) {
    int n;
    utf8_at(s, i, &n);
    i += n;
    k--;
  }
  return i;
}
// Unicode's White_Space, as Rust's `char::is_whitespace`.
static int is_space(int32_t c) {
  return (c >= 9 && c <= 13) || c == 32 || c == 0x85 || c == 0xA0 || c == 0x1680 || (c >= 0x2000 && c <= 0x200A) || c == 0x2028 || c == 0x2029 || c == 0x202F || c == 0x205F || c == 0x3000;
}

Str *strata_str_trim(Str *s) {
  int64_t a = 0, b = s->len;
  while (a < b) {
    int n;
    if (!is_space(utf8_at(s, a, &n))) break;
    a += n;
  }
  while (b > a) {
    int64_t p = b - 1;
    while ((s->bytes[p] & 0xC0) == 0x80) p--;
    int n;
    if (!is_space(utf8_at(s, p, &n))) break;
    b = p;
  }
  return str_from(s->bytes + a, b - a);
}

// The bytes from offset `a` to `b`, both clamped to 0..len; an offset inside
// a character's bytes moves back to its start (never a fault).
static int64_t char_start(Str *s, int64_t i) {
  while (i > 0 && i < s->len && ((unsigned char)s->bytes[i] & 0xC0) == 0x80) i--;
  return i;
}

Str *strata_str_byte_slice(Str *s, int64_t a, int64_t b) {
  if (a < 0) a = 0;
  if (b > s->len) b = s->len;
  if (a > s->len) a = s->len;
  a = char_start(s, a);
  b = char_start(s, b);
  if (b <= a) return str_from("", 0);
  return str_from(s->bytes + a, b - a);
}

// Character `k`, or NULL when there is none.
Str *strata_str_at(Str *s, int64_t k) {
  if (k < 0) return NULL;
  int64_t i = char_offset(s, k);
  if (i >= s->len) return NULL;
  int n;
  utf8_at(s, i, &n);
  return str_from(s->bytes + i, n);
}

// `xs[i]` on a string: character `k`, or a fault.
Str *strata_str_at_or_fault(Str *s, int64_t k) {
  Str *c = strata_str_at(s, k);
  if (c) return c;
  char buf[96];
  snprintf(buf, sizeof buf, "index %lld out of range (length %lld)", (long long)k, (long long)strata_str_len(s));
  soft_fault(4, str_c(buf));
  return NO_STR;
}

// Characters `a` to `b`, clamped to the string (as in Python).
Str *strata_str_slice(Str *s, int64_t a, int64_t b) {
  if (a < 0) a = 0;
  if (b <= a) return str_from("", 0);
  int64_t i = char_offset(s, a), j = i;
  for (int64_t k = a; k < b && j < s->len; k++) {
    int n;
    utf8_at(s, j, &n);
    j += n;
  }
  return str_from(s->bytes + i, j - i);
}

// Whether `c` is in a table of ranges (pairs of first and last).
static int in_ranges(int32_t c, const int32_t *r, int64_t n) {
  int64_t lo = 0, hi = n;
  while (lo < hi) {
    int64_t mid = (lo + hi) / 2;
    if (c < r[2 * mid]) hi = mid;
    else if (c > r[2 * mid + 1]) lo = mid + 1;
    else return 1;
  }
  return 0;
}

// The mapping of `c` in a case table (rows of a character and the up to
// three it becomes), or NULL when it maps to itself.
static const int32_t *case_row(int32_t c, const int32_t *t, int64_t n) {
  int64_t lo = 0, hi = n;
  while (lo < hi) {
    int64_t mid = (lo + hi) / 2;
    if (c < t[4 * mid]) hi = mid;
    else if (c > t[4 * mid]) lo = mid + 1;
    else return t + 4 * mid + 1;
  }
  return NULL;
}

// `to_upper` / `to_lower`, with Rust's mappings (`table`). Lowercase `Σ` is
// `ς` at the end of a word: after a cased character (`cased`), and not
// before one.
Str *strata_str_case(Str *s, const int32_t *table, int64_t n, const int32_t *cased, int64_t m, _Bool lower) {
  Str *o = str_alloc(s->len * 3);
  char *p = o->bytes;
  int32_t prev = -1;
  int64_t i = 0;
  while (i < s->len) {
    int k;
    int32_t c = utf8_at(s, i, &k);
    if (lower && c == 0x3A3) {
      int final = prev >= 0 && in_ranges(prev, cased, m);
      if (final && i + k < s->len) {
        int k2;
        final = !in_ranges(utf8_at(s, i + k, &k2), cased, m);
      }
      p += utf8_put(final ? 0x3C2 : 0x3C3, p);
    } else {
      const int32_t *row = case_row(c, table, n);
      if (!row) p += utf8_put(c, p);
      else
        for (int j = 0; j < 3 && row[j]; j++) p += utf8_put(row[j], p);
    }
    prev = c;
    i += k;
  }
  o->len = p - o->bytes;
  return o;
}

// `is_digit` (0), `is_alpha` (1, `ranges` from Unicode's Alphabetic),
// `is_space` (2): not empty, and every character of the class.
_Bool strata_str_is(Str *s, int64_t class, const int32_t *ranges, int64_t n) {
  if (s->len == 0) return 0;
  int64_t i = 0;
  while (i < s->len) {
    int k;
    int32_t c = utf8_at(s, i, &k);
    int ok = class == 0 ? (c >= '0' && c <= '9') : class == 1 ? in_ranges(c, ranges, n) : is_space(c);
    if (!ok) return 0;
    i += k;
  }
  return 1;
}

// A string as Rust's `{:?}` writes it, in messages.
static Str *debug_str(Str *s) {
  Str *o = str_alloc(s->len * 10 + 2);
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
    case 0: p += sprintf(p, "\\0"); break;
    default:
      if (c < 0x20 || c == 0x7F) p += sprintf(p, "\\u{%x}", c);
      else *p++ = (char)c;
    }
  }
  *p++ = '"';
  o->len = p - o->bytes;
  return o;
}

int64_t strata_str_ord(Str *s) {
  int n;
  if (s->len > 0) {
    int32_t c = utf8_at(s, 0, &n);
    if (n == s->len) return c;
  }
  soft_fault(4, strata_concat_owned(str_c("ord needs a single character, got "), debug_str(s)));
  return 0;
}

// A string's length in UTF-8 bytes, and its byte `i` (0 to 255): what the
// prelude's `bytes()` and `code_points()` read, in Strata.
int64_t strata_str_byte_len(Str *s) { return s->len; }

int64_t strata_str_byte_at(Str *s, int64_t i) {
  if (i < 0 || i >= s->len) {
    char buf[96];
    snprintf(buf, sizeof buf, "byte %lld out of range (%lld bytes)", (long long)i, (long long)s->len);
    soft_fault(4, str_c(buf));
    return 0;
  }
  return (unsigned char)s->bytes[i];
}

Str *strata_str_chr(int64_t c) {
  if (c < 0 || c > 0x10FFFF || (c >= 0xD800 && c <= 0xDFFF)) {
    char buf[64];
    snprintf(buf, sizeof buf, "chr: %lld is not a character", (long long)c);
    soft_fault(4, str_c(buf));
    return NO_STR;
  }
  char b[4];
  return str_from(b, utf8_put((int32_t)c, b));
}
#endif

#ifndef STRATA_CORE_RT
// `parse_int`, as Rust's `BigInt` reads the trimmed string: a sign, then a
// digit, then digits and `_`. False when it isn't a number; a fault when it
// doesn't fit in 64 bits (big integers are not compiled yet).
static _Bool parse_int_trimmed(Str *t, int64_t *out);

_Bool strata_parse_int(Str *s, int64_t *out) {
  Str *t = strata_str_trim(s);
  _Bool ok = parse_int_trimmed(t, out);
  str_drop(t);
  return ok;
}
#endif

// (the C library's with core.rt too, as `strata_show_float`)
static _Bool parse_float_trimmed(Str *t, double *out);

_Bool strata_parse_float(Str *s, double *out) {
  Str *t = strata_str_trim(s);
  _Bool ok = parse_float_trimmed(t, out);
  str_drop(t);
  return ok;
}

#ifndef STRATA_CORE_RT
static _Bool parse_int_trimmed(Str *t, int64_t *out) {
  int64_t i = 0, n = t->len;
  int neg = 0;
  if (i < n && (t->bytes[i] == '+' || t->bytes[i] == '-')) neg = t->bytes[i++] == '-';
  if (i >= n || t->bytes[i] < '0' || t->bytes[i] > '9') return 0;
  Num v = {1, 0, digits(n / 9 + 2)};
  for (; i < n; i++) {
    char c = t->bytes[i];
    if (c == '_') continue;
    if (c < '0' || c > '9') {
      free(v.d);
      return 0;
    }
    uint64_t carry = (uint64_t)(c - '0');
    for (int64_t k = 0; k < v.n; k++) {
      carry += (uint64_t)v.d[k] * 10;
      v.d[k] = (uint32_t)carry;
      carry >>= 32;
    }
    if (carry) v.d[v.n++] = (uint32_t)carry;
  }
  v.sign = v.n ? (neg ? -1 : 1) : 0;
  *out = int_of(v);
  return 1;
}
#endif

// `parse_float`, as Rust's `f64::from_str` reads the trimmed string (no hex,
// no `_`; `inf` and `NaN` are not finite, so not numbers either).
static _Bool parse_float_trimmed(Str *t, double *out) {
  int64_t i = 0, n = t->len, digits = 0;
  const char *b = t->bytes;
  if (i < n && (b[i] == '+' || b[i] == '-')) i++;
  while (i < n && b[i] >= '0' && b[i] <= '9') i++, digits++;
  if (i < n && b[i] == '.') {
    i++;
    while (i < n && b[i] >= '0' && b[i] <= '9') i++, digits++;
  }
  if (digits == 0) return 0;
  if (i < n && (b[i] == 'e' || b[i] == 'E')) {
    i++;
    if (i < n && (b[i] == '+' || b[i] == '-')) i++;
    int64_t e = 0;
    while (i < n && b[i] >= '0' && b[i] <= '9') i++, e++;
    if (e == 0) return 0;
  }
  if (i != n) return 0;
  double x = strtod(cstr(t), NULL);
  if (!isfinite(x)) return 0;
  *out = x;
  return 1;
}

// ---- f-string format specs (`interp::format_spec`) ----
//
// `strata_format` is core.fmt's when core.rt is linked in (`STRATA_CORE_RT`).

// A float with `p` decimals, as Rust's `{:.p}` (the C library's with
// core.rt too, as `strata_show_float`).
Str *strata_show_float_fixed(double x, int64_t p) {
  int n = snprintf(NULL, 0, "%.*f", (int)p, x);
  Str *s = str_alloc(n);
  char *tmp = malloc((size_t)n + 1);
  snprintf(tmp, (size_t)n + 1, "%.*f", (int)p, x);
  memcpy(s->bytes, tmp, (size_t)n);
  free(tmp);
  return s;
}

#ifndef STRATA_CORE_RT
// `body` padded to `width` characters: `fill` and `align` (`<`, `>`, `^`, or
// 0 for the default: numbers right, the rest left), `+` for a number's sign,
// `0` to pad a number with zeros after its sign.
// `body` is handed over.
Str *strata_format(Str *body, _Bool numeric, int32_t fill, int32_t align, _Bool plus, _Bool zero, int64_t width) {
  if (plus && !(body->len > 0 && body->bytes[0] == '-')) body = strata_concat_owned(str_c("+"), body);
  int64_t n = strata_str_len(body);
  if (n >= width) return body;
  int64_t pad = width - n;
  if (zero && align == 0) {
    int sign = body->len > 0 && (body->bytes[0] == '+' || body->bytes[0] == '-');
    Str *o = str_alloc(body->len + pad);
    memcpy(o->bytes, body->bytes, (size_t)sign);
    memset(o->bytes + sign, '0', (size_t)pad);
    memcpy(o->bytes + sign + pad, body->bytes + sign, (size_t)(body->len - sign));
    str_drop(body);
    return o;
  }
  char f[4];
  int fl = utf8_put(fill, f);
  if (align == 0) align = numeric ? '>' : '<';
  int64_t left = align == '<' ? 0 : align == '>' ? pad : pad / 2, right = pad - left;
  Str *o = str_alloc(body->len + pad * fl);
  char *p = o->bytes;
  for (int64_t i = 0; i < left; i++) memcpy(p, f, (size_t)fl), p += fl;
  memcpy(p, body->bytes, (size_t)body->len), p += body->len;
  for (int64_t i = 0; i < right; i++) memcpy(p, f, (size_t)fl), p += fl;
  str_drop(body);
  return o;
}
#endif

// ---- `extern python`: a Python worker ----
//
// std.python calls Python functions through one worker process for the whole
// program: `PYTHON -c DRIVER PATH` (the driver below), started on the first
// call, speaking JSON lines over its standard input and output: a request
// `{"module": m, "function": f, "args": [...]}`, a reply `{"ok": value}`,
// `{"error": {"type", "message", "traceback"}}` (an exception), or
// `{"unencodable": message}` (a result with no JSON form: a set, a NaN).
// Calls from several tasks (threads) take turns (`py_lock`): one request and its reply at a time. A
// worker that ends (killed, `os._exit`, a crash) fails the call it was
// answering (`{"failed": message}`, made here) and the next call starts a
// new one. Its standard error is the program's (what Python prints goes
// there: the driver points `sys.stdout` at it, keeping the real one for
// replies). The worker ends when this process does (its input closes).
static const char *py_driver =
  "import sys, json, importlib, traceback\n"
  "out = sys.stdout\n"
  "sys.stdout = sys.stderr\n"
  "for p in reversed([x for x in sys.argv[1].split(':') if x] if len(sys.argv) > 1 else []):\n"
  "    sys.path.insert(0, p)\n"
  "while True:\n"
  "    line = sys.stdin.readline()\n"
  "    if not line:\n"
  "        break\n"
  "    try:\n"
  "        req = json.loads(line)\n"
  "        f = getattr(importlib.import_module(req['module']), req['function'])\n"
  "        result = f(*req['args'])\n"
  "    except (KeyboardInterrupt, SystemExit):\n"
  "        raise\n"
  "    except BaseException as e:\n"
  "        reply = json.dumps({'error': {'type': type(e).__name__, 'message': str(e), 'traceback': traceback.format_exc()}})\n"
  "    else:\n"
  "        try:\n"
  "            reply = json.dumps({'ok': result}, allow_nan=False)\n"
  "        except (TypeError, ValueError, RecursionError) as e:\n"
  "            reply = json.dumps({'unencodable': str(e)})\n"
  "    out.write(reply + '\\n')\n"
  "    out.flush()\n";

static pthread_mutex_t py_lock = PTHREAD_MUTEX_INITIALIZER;
static pid_t py_pid = -1;
static int py_to = -1, py_from = -1;
// (what was read and not yet handed back)
static char *py_buf = NULL;
static size_t py_len = 0, py_cap = 0;
// (the reply handed back: each thread's own, kept until its next call, so
// the caller copies it after the lock is let go)
static __thread char *py_reply = NULL;

static const char *py_give(const char *s) {
  free(py_reply);
  py_reply = strdup(s);
  return py_reply;
}

// `{"failed": "MESSAGE"}`, the message escaped as JSON.
static const char *py_failed(const char *why) {
  char esc[2600];
  size_t k = 0;
  for (const char *p = why; *p && k < sizeof esc - 8; p++) {
    unsigned char c = (unsigned char)*p;
    if (c == '"' || c == '\\') esc[k++] = '\\', esc[k++] = (char)c;
    else if (c == '\n') esc[k++] = '\\', esc[k++] = 'n';
    else if (c < 0x20) k += (size_t)snprintf(esc + k, 7, "\\u%04x", c);
    else esc[k++] = (char)c;
  }
  esc[k] = 0;
  char *out = malloc(k + 16);
  snprintf(out, k + 16, "{\"failed\":\"%s\"}", esc);
  free(py_reply);
  py_reply = out;
  return py_reply;
}

// The worker gone: its pipes closed, the process reaped; how it ended, in
// `why`.
static void py_stop(char *why, size_t n) {
  if (py_to >= 0) close(py_to);
  if (py_from >= 0) close(py_from);
  py_to = py_from = -1;
  py_len = 0;
  snprintf(why, n, "the Python worker ended");
  if (py_pid > 0) {
    int st = 0;
    if (waitpid(py_pid, &st, 0) == py_pid) {
      if (WIFEXITED(st)) snprintf(why, n, "the Python worker ended (exit code %d)", WEXITSTATUS(st));
      else if (WIFSIGNALED(st)) snprintf(why, n, "the Python worker was killed (signal %d)", WTERMSIG(st));
    }
  }
  py_pid = -1;
}

static int py_start(const char *python, const char *path, char *why, size_t n) {
  int in[2], out[2];
  if (cloexec_pipe(in) != 0) return snprintf(why, n, "can't start the Python worker: %s", strerror(errno)), -1;
  if (cloexec_pipe(out) != 0) {
    snprintf(why, n, "can't start the Python worker: %s", strerror(errno));
    close(in[0]);
    close(in[1]);
    return -1;
  }
  // (only its standard input, output and error: see `strata_host_run`)
  posix_spawnattr_t at;
  posix_spawnattr_init(&at);
  posix_spawnattr_setflags(&at, POSIX_SPAWN_CLOEXEC_DEFAULT);
  posix_spawn_file_actions_t fa;
  posix_spawn_file_actions_init(&fa);
  posix_spawn_file_actions_adddup2(&fa, in[0], 0);
  posix_spawn_file_actions_adddup2(&fa, out[1], 1);
  posix_spawn_file_actions_addinherit_np(&fa, 2);
  char *argv[] = {(char *)python, "-c", (char *)py_driver, (char *)path, NULL};
  extern char **environ;
  pid_t pid;
  int e = posix_spawnp(&pid, python, &fa, &at, argv, environ);
  posix_spawn_file_actions_destroy(&fa);
  posix_spawnattr_destroy(&at);
  close(in[0]);
  close(out[1]);
  if (e != 0) {
    close(in[1]);
    close(out[0]);
    snprintf(why, n, "can't start `%s`: %s", python, strerror(e));
    return -1;
  }
#ifdef F_SETNOSIGPIPE
  // (a worker that died breaks its pipe: writing fails with EPIPE, no SIGPIPE)
  fcntl(in[1], F_SETNOSIGPIPE, 1);
#endif
  py_pid = pid;
  py_to = in[1];
  py_from = out[0];
  py_len = 0;
  return 0;
}

// Sends `request` (one line of JSON) to the worker, started first with
// `python` and the module folders `path` (`:`-separated) if there is none,
// and gives its reply line, or `{"failed": message}`.
const char *python_worker_call(const char *python, const char *path, const char *request) {
  pthread_mutex_lock(&py_lock);
  char why[700];
  if (py_pid < 0 && py_start(python, path, why, sizeof why) != 0) {
    pthread_mutex_unlock(&py_lock);
    return py_failed(why);
  }
  size_t len = strlen(request), sent = 0;
  _Bool ok = 1;
  while (ok && sent <= len) {
    // (the request, then its newline)
    const char *p = sent < len ? request + sent : "\n";
    size_t m = sent < len ? len - sent : 1;
    ssize_t w = write(py_to, p, m);
    if (w < 0 && errno == EINTR) continue;
    if (w <= 0) ok = 0;
    else sent += (size_t)w;
  }
  if (!ok) {
    py_stop(why, sizeof why);
    pthread_mutex_unlock(&py_lock);
    return py_failed(why);
  }
  for (;;) {
    char *nl = py_len ? memchr(py_buf, '\n', py_len) : NULL;
    if (nl) {
      size_t k = (size_t)(nl - py_buf);
      py_buf[k] = 0;
      const char *r = py_give(py_buf);
      memmove(py_buf, nl + 1, py_len - k - 1);
      py_len -= k + 1;
      pthread_mutex_unlock(&py_lock);
      return r;
    }
    if (py_cap - py_len < 4096) {
      py_cap = py_cap ? py_cap * 2 : 65536;
      py_buf = realloc(py_buf, py_cap);
    }
    ssize_t r = read(py_from, py_buf + py_len, py_cap - py_len - 1);
    if (r < 0 && errno == EINTR) continue;
    if (r <= 0) {
      py_stop(why, sizeof why);
      pthread_mutex_unlock(&py_lock);
      return py_failed(why);
    }
    py_len += (size_t)r;
  }
}
