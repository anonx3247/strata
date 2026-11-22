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
#include <math.h>
#include <setjmp.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/stat.h>
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


static int64_t allocated, freed;

// While a task runs (`spawn`), threads may hold the same objects, so counts
// change atomically; with none alive, they don't. Either way a count of 1
// means only this thread can see the object. Set before the first task's
// thread starts and cleared after the last one is joined, so it never
// changes while another thread reads it.
static int threaded;
static int live_tasks;
static pthread_mutex_t made_lock = PTHREAD_MUTEX_INITIALIZER;
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

static void set_threaded(int v) {
  __atomic_store_n(&threaded, v, __ATOMIC_RELAXED);
  __atomic_store_n(&strata_slow_counts, v || checking, __ATOMIC_RELAXED);
}

// In check mode, every object made (to list those still alive at the end).
static int64_t **made;
static int64_t nmade, capmade;

// The count word of an object.
static int64_t *count_of(void *p) { return (int64_t *)p - 1; }

static _Noreturn void freed_again(const char *what) {
  fflush(stdout);
  fprintf(stderr, "memory: %s of a freed object\n", what);
  abort();
}

// A new object of `n` bytes, held once.
void *strata_alloc(int64_t n) {
  int64_t *c = malloc((size_t)n + 8);
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
  if (checking) {
    memset(p, 0xDB, usable_size(c) - 8);
    *c = FREED;
    return;
  }
  free(c);
}

// A string has nothing in it to drop.
static void str_drop(void *s) {
  if (strata_release(s)) strata_free(s);
}

// Whether `p` has a single holder, so it may be changed in place.
_Bool strata_unique(void *p) { return THREADED ? __atomic_load_n(count_of(p), __ATOMIC_ACQUIRE) == 1 : *count_of(p) == 1; }

// With STRATA_MEMORY set, how many objects the program made and freed.
// In check mode, the first few still alive: their count, size and first
// word (a block's tag, a string's or a list's length), as text if it reads
// as a string.
static void memory_stats(void) {
  fprintf(stderr, "memory: %lld allocated, %lld freed, %lld live\n", (long long)allocated, (long long)freed, (long long)(allocated - freed));
  int shown = 0;
  for (int64_t i = 0; i < nmade && shown < 20; i++) {
    int64_t *c = made[i];
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

// ---- faults ----

// The functions being run, outermost first (as the interpreter's call
// stack): a fault shows them. A recursion is one run of the same function
// (`down (95846 times)`), so a deep one costs nothing to keep; only the
// first FRAMES runs are kept.
#define FRAMES 4096
static _Thread_local Str *frames[FRAMES];
static _Thread_local int64_t counts[FRAMES];
static _Thread_local int64_t runs;  // runs in use, counting those past FRAMES

void strata_enter(Str *name) {
  if (runs > 0 && runs <= FRAMES && frames[runs - 1] == name) {
    counts[runs - 1]++;
    return;
  }
  if (runs < FRAMES) {
    frames[runs] = name;
    counts[runs] = 1;
  }
  runs++;
}

void strata_leave(void) {
  if (runs <= FRAMES && --counts[runs - 1] > 0) return;
  runs--;
}

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

static char *stack_text(void) { return stack_from(frames, counts, runs); }

static void print_stack(void) {
  if (runs == 0) return;
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
static _Thread_local uint64_t rng = 7;  // the generators' state (below)
static _Thread_local uint64_t test_seed = 7;  // where a test's numbers start
static _Thread_local sigjmp_buf *trap;
static void catch_overflow(void);
static _Thread_local int32_t fault_kind;
static _Thread_local Str *fault_msg;
static _Thread_local char *fault_stack, *printed;
static _Thread_local size_t printed_len, printed_cap;
static _Thread_local _Bool capturing;

static void keep_printed(const char *b, size_t n) {
  if (printed_len + n + 1 > printed_cap) printed = realloc(printed, printed_cap = (printed_len + n + 1) * 2);
  memcpy(printed + printed_len, b, n);
  printed_len += n;
  printed[printed_len] = 0;
}


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

int32_t strata_test(void (*f)(void)) {
  sigjmp_buf here;
  catch_overflow();
  runs = 0;
  rng = test_seed;
  forget_senders();
  printed_len = 0;
  if (printed) printed[0] = 0;
  capturing = 1;
  Contained run = {0}, *outer = contained;
  contained = &run;
  // its own run: its threads' waits are counted apart from the program's
  Run trun = {0}, *outer_run = my_run;
  Waiter *outer_me = me;
  trun.test = &run;
  trun.root.frames = frames;
  trun.root.counts = counts;
  trun.root.runs = &runs;
  add_waiter(&trun, &trun.root);
  my_run = &trun;
  me = &trun.root;
  trap = &here;
  int32_t r = 1;
  if (sigsetjmp(here, 1) == 0) {
    f();
    r = 0;
  }
  trap = NULL;
  capturing = 0;
  contained = outer;
  my_run = outer_run;
  me = outer_me;
  // a panic of the run itself (a deadlock): the test fails with it
  free(trun.vtime);
  if (r == 0 && run.panicked && run.msg) {
    r = 1;
    fault_kind = run.kind;
    fault_msg = run.msg;
    free(fault_stack);
    fault_stack = run.stack;
  }
  if (run.panicked) __atomic_sub_fetch(&strata_cancel_any, 1, __ATOMIC_RELAXED);
  return r;
}

// ---- fuzzing (`strata verify --jit`) ----
//
// A fuzz driver (llvm.rs, `compile_fuzz`) generates a function's arguments,
// hands each to strata (`strata_fuzz_keep`, src/llvm/jit.rs: read by its
// shape), and calls it; `strata_fuzz` runs one case as
// `strata_test` runs a test, with the generators seeded and a step budget
// (each loop iteration and call is a step): past it, a `Limit` fault.

_Noreturn void strata_fault(int32_t kind, Str *msg);

static _Thread_local int64_t steps;
static _Thread_local _Bool counting;

void strata_step(void) {
  if (counting && --steps < 0) {
    counting = 0;
    strata_fault(6, str_c("too many steps"));
  }
}

int32_t strata_fuzz(void (*f)(void), int64_t seed, int64_t budget) {
  steps = budget;
  counting = 1;
  test_seed = (uint64_t)seed;
  int32_t r = strata_test(f);
  test_seed = 7;
  counting = 0;
  return r;
}

void strata_seed(int64_t seed) { rng = (uint64_t)seed; }

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

// Values the host wrote into memory for compiled code to take
// (`strata_run_call`'s arguments; llvm.rs, `Entry::Call`).
static _Thread_local int64_t *given;
void strata_set_given(int64_t *g) { given = g; }
int64_t strata_given(int64_t i) { return given[i]; }

// Compile-time code's value (`strata.ct.N` hands it over), for the compiler
// to read (src/llvm/native.rs).
static _Thread_local int64_t ct_kept;
void strata_ct_keep(int64_t v) { ct_kept = v; }
int64_t strata_ct_kept(void) { return ct_kept; }

// The last test's fault: its kind, message and call stack, and what it printed.
const char *strata_test_kind(void) { return KINDS[fault_kind & 255]; }
Str *strata_test_message(void) { return fault_msg; }
const char *strata_test_stack(void) { return fault_stack ? fault_stack : ""; }
const char *strata_test_printed(void) { return printed ? printed : ""; }

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

static _Noreturn void fault_at(int32_t kind, Str *msg, char *task_stack) {
  char *stack = NULL;
  if (!(kind & 256)) {
    char *here = stack_text();
    if (!task_stack && task_origin && !trap) {
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
  if (trap) {
    // (a test's tasks stop, rather than wait for what won't come; a
    // supervised task's panic is kept for its result)
    if (contained && contained->supervised) record_panic(contained, kind & 255, msg, (kind & 256) ? strdup("") : panic_stack());
    else if (contained) panic_contained(contained);
    finish_open_scopes();
    fault_kind = kind;
    fault_msg = msg;
    free(fault_stack);
    fault_stack = stack;
    siglongjmp(*trap, 1);
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
    r->root.frames = frames;
    r->root.counts = counts;
    r->root.runs = &runs;
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
  rng = t->seed;
  capturing = t->capturing;
  counting = t->counting;
  steps = t->steps;
  given = t->given;
  for (int64_t i = 0; i < t->nshares; i++) hold_sender(t->shares[i]);
  self_task = t;
  contained = t->contained;
  task_origin = t->origin;
  unwound = 0;
  pthread_mutex_lock(&chan_lock);
  my_run = t->run_of;
  me = &t->w;
  t->w.frames = frames;
  t->w.counts = counts;
  t->w.runs = &runs;
  pthread_mutex_unlock(&chan_lock);
  // outside a test a panic ends the program (no trap); inside one it ends the task
  sigjmp_buf here;
  trap = contained ? &here : NULL;
  if (!contained || sigsetjmp(here, 1) == 0) {
    t->run(t->clo, &t->out);
    t->unwound = unwound;
  } else if (!t->fail) {
    t->faulted = 1;
    t->kind = fault_kind;
    t->msg = fault_msg;
    t->stack = fault_stack;
    fault_stack = NULL;
    panic_contained(contained);
  }
  trap = NULL;
  // a supervised task that panicked: its result is the panic
  _Bool died = t->fail && t->boundary.panicked;
  t->printed = printed;
  t->printed_len = printed_len;
  printed = NULL;
  printed_len = printed_cap = 0;
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
  rng = rng * 6364136223846793005ULL + 1442695040888963407ULL;
  t->seed = rng ^ (rng >> 29);
  t->capturing = capturing;
  t->counting = counting;
  t->steps = steps;
  t->given = given;
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
    if (capturing) keep_printed(t->printed, t->printed_len);
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
  *count_of(c) = 2;
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
  rng = rng * 6364136223846793005ULL + 1442695040888963407ULL;
  int64_t start = (int64_t)((rng >> 33) % (uint64_t)n);
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
  trap = NULL;
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
// An `Int` never overflows (as the interpreter's): one in [-2^62, 2^62) is
// itself, and any other is a big integer, a pointer kept in the 64-bit
// values outside that range: above it for positive ones, below for
// negative ones, so comparing a small and a big integer as 64-bit values
// is right. The generated code does small arithmetic itself and calls
// these when an operand or the result isn't small. Big integers are never
// freed (they are rare: a program that overflows).

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

// One holder more, or fewer, of a big integer (the generated code tests
// whether an `Int` is big before calling these).
void strata_int_dup(int64_t v) {
  if (is_big(v)) strata_dup(big_at(v));
}

void strata_int_drop(int64_t v) {
  if (is_big(v) && strata_release(big_at(v))) strata_free(big_at(v));
}

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

int64_t strata_div(int64_t a, int64_t b) {
  if (b == 0 && soft_fault(4, str_c("division by zero"))) return 0;
  if (!is_big(a) && !is_big(b)) {
    // Euclidean: the remainder is never negative
    int64_t q = a / b, r = a % b;
    if (r < 0) q = b > 0 ? q - 1 : q + 1;
    return is_big(q) ? int_of(num_of(q)) : q;
  }
  Num x = num_of(a), y = num_of(b);
  int64_t q, r;
  num_divmod(x, y, &q, &r);
  free(x.d), free(y.d);
  strata_int_drop(r);
  return q;
}

int64_t strata_mod(int64_t a, int64_t b) {
  if (b == 0 && soft_fault(4, str_c("division by zero"))) return 0;
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
    Str *m = big_show(v);
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

// ---- foreign calls (`extern c`) ----
//
// What the generated code converts a value with, on its way to C and back.

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

static uint64_t next_random(void) {
  uint64_t z = (rng += UINT64_C(0x9E3779B97F4A7C15));
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

_Noreturn void strata_overflow(void) {
  strata_fault(4, str_c("integer overflow"));
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

// ---- lists ----
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

List *strata_range(int64_t lo, int64_t hi) {
  int64_t n = hi > lo ? hi - lo : 0;
  List *l = strata_list_new(n);
  for (int64_t i = 0; i < n; i++) l->buf->data[i] = lo + i;
  l->buf->used = n;
  l->len = n;
  return l;
}

// `main` returned `Err(e)`.
void strata_main_error(Str *shown) {
  fflush(stdout);
  fprintf(stderr, "error: %.*s\n", (int)shown->len, shown->bytes);
}

// ---- output ----

void strata_print(Str *s) {
  if (capturing) {
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
  runs = 0;
  rng = 7;
  catch_overflow();
  prog_argc = argc > 0 ? argc - 1 : 0;
  prog_argv = argv + 1;
  if (argc > 0 && argv[0]) prog_name = argv[0];
  signal(SIGPIPE, SIG_IGN);
  // (what compile-time code made before, in this process, isn't the program's)
  allocated = freed = 0;
  nmade = 0;
  static _Bool registered;
  const char *m = getenv("STRATA_MEMORY");
  checking = m && !strcmp(m, "check");
  set_threaded(THREADED);
  if (m && !registered) registered = 1, atexit(memory_stats);
}

Str *strata_prog_name(void) {
  const char *slash = strrchr(prog_name, '/');
  return str_c(slash ? slash + 1 : prog_name);
}

// The options could not be read: the help asked for (on standard output), or
// what is wrong with the arguments (on standard error).
void strata_options_error(_Bool help, Str *text) {
  if (help) {
    fwrite(text->bytes, 1, (size_t)text->len, stdout);
    fflush(stdout);
  } else {
    fflush(stdout);
    fprintf(stderr, "error: %.*s\n", (int)text->len, text->bytes);
  }
}

// A C copy of `s`, for the OS: valid until the next call (one buffer, reused).
static char *cstr(Str *s) {
  static char *c;
  static size_t cap;
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
  char buf[300];
  snprintf(buf, sizeof buf, "%s (os error %d)", strerror(e), e);
  *kind = str_c(k);
  *msg = str_c(buf);
}

void strata_host_write(int64_t fd, Str *text) {
  // in a test, the terminal's output is the test's own, as `print`'s is
  if (capturing) {
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

// ---- strings ----
//
// The string built-ins (`interp::Interp::builtin`). Strings are UTF-8 and
// indexed by character (code point). What depends on Unicode's tables (case
// mappings, `is_alpha`) takes a table the compiler made from Rust's own
// (`llvm.rs`, `unicode_table`), so both agree.

// The character at byte `i` (valid UTF-8), and its length in bytes.
static int32_t utf8_at(Str *s, int64_t i, int *n) {
  unsigned char c = (unsigned char)s->bytes[i];
  if (c < 0x80) return *n = 1, c;
  if (c < 0xE0) return *n = 2, ((c & 0x1F) << 6) | (s->bytes[i + 1] & 0x3F);
  if (c < 0xF0) return *n = 3, ((c & 0x0F) << 12) | ((s->bytes[i + 1] & 0x3F) << 6) | (s->bytes[i + 2] & 0x3F);
  return *n = 4, ((c & 0x07) << 18) | ((s->bytes[i + 1] & 0x3F) << 12) | ((s->bytes[i + 2] & 0x3F) << 6) | (s->bytes[i + 3] & 0x3F);
}

static int utf8_put(int32_t c, char *o) {
  if (c < 0x80) return o[0] = (char)c, 1;
  if (c < 0x800) return o[0] = (char)(0xC0 | (c >> 6)), o[1] = (char)(0x80 | (c & 0x3F)), 2;
  if (c < 0x10000) return o[0] = (char)(0xE0 | (c >> 12)), o[1] = (char)(0x80 | ((c >> 6) & 0x3F)), o[2] = (char)(0x80 | (c & 0x3F)), 3;
  o[0] = (char)(0xF0 | (c >> 18)), o[1] = (char)(0x80 | ((c >> 12) & 0x3F)), o[2] = (char)(0x80 | ((c >> 6) & 0x3F)), o[3] = (char)(0x80 | (c & 0x3F));
  return 4;
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

// The first place `sub` occurs in `s` at or after byte `from`, or -1.
static int64_t find_from(Str *s, Str *sub, int64_t from) {
  for (int64_t i = from; i + sub->len <= s->len; i++)
    if (memcmp(s->bytes + i, sub->bytes, (size_t)sub->len) == 0) return i;
  return -1;
}

static List *push_str(List *l, Str *s) { return append(l, (int64_t)(intptr_t)s); }

List *strata_str_split(Str *s, Str *sep) {
  if (sep->len == 0 && soft_fault(4, str_c("split: empty separator"))) return strata_list_new(0);
  List *l = strata_list_new(4);
  int64_t start = 0, at;
  while ((at = find_from(s, sep, start)) >= 0) {
    l = push_str(l, str_from(s->bytes + start, at - start));
    start = at + sep->len;
  }
  return push_str(l, str_from(s->bytes + start, s->len - start));
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

_Bool strata_str_contains(Str *s, Str *sub) { return find_from(s, sub, 0) >= 0; }

_Bool strata_str_starts_with(Str *s, Str *p) { return p->len <= s->len && memcmp(s->bytes, p->bytes, (size_t)p->len) == 0; }

_Bool strata_str_ends_with(Str *s, Str *p) { return p->len <= s->len && memcmp(s->bytes + s->len - p->len, p->bytes, (size_t)p->len) == 0; }

// The byte offset of the first match of `sub`, or -1.
int64_t strata_str_index_of(Str *s, Str *sub) {
  return find_from(s, sub, 0);
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

Str *strata_str_reverse(Str *s) {
  Str *o = str_alloc(s->len);
  int64_t i = 0;
  while (i < s->len) {
    int n;
    utf8_at(s, i, &n);
    memcpy(o->bytes + s->len - i - n, s->bytes + i, (size_t)n);
    i += n;
  }
  return o;
}

List *strata_str_chars(Str *s) {
  List *l = strata_list_new(s->len);
  int64_t i = 0;
  while (i < s->len) {
    int n;
    utf8_at(s, i, &n);
    l = push_str(l, str_from(s->bytes + i, n));
    i += n;
  }
  return l;
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

Str *strata_str_replace(Str *s, Str *from, Str *to) {
  if (from->len == 0 && soft_fault(4, str_c("replace: empty pattern"))) return NO_STR;
  Str *out = str_from("", 0);
  int64_t start = 0, at;
  while ((at = find_from(s, from, start)) >= 0) {
    out = strata_concat_owned(out, str_from(s->bytes + start, at - start));
    Str *next = strata_concat(out, to);
    str_drop(out);
    out = next;
    start = at + from->len;
  }
  return strata_concat_owned(out, str_from(s->bytes + start, s->len - start));
}

// A count that is not negative is accepted: a string too large to hold is
// out of memory, as when it is built any other way.
Str *strata_str_repeat(Str *s, int64_t n) {
  if (n < 0) {
    char buf[64];
    snprintf(buf, sizeof buf, "repeat: negative count %lld", (long long)n);
    soft_fault(4, str_c(buf));
    return NO_STR;
  }
  if (s->len > 0 && n > (INT64_MAX - 64) / s->len) {
    fputs("fault (Limit): out of memory\n", stderr);
    exit(101);
  }
  Str *o = str_alloc(s->len * n);
  for (int64_t i = 0; i < n; i++) memcpy(o->bytes + i * s->len, s->bytes, (size_t)s->len);
  return o;
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

// `parse_int`, as Rust's `BigInt` reads the trimmed string: a sign, then a
// digit, then digits and `_`. False when it isn't a number; a fault when it
// doesn't fit in 64 bits (big integers are not compiled yet).
static _Bool parse_int_trimmed(Str *t, int64_t *out);
static _Bool parse_float_trimmed(Str *t, double *out);

_Bool strata_parse_int(Str *s, int64_t *out) {
  Str *t = strata_str_trim(s);
  _Bool ok = parse_int_trimmed(t, out);
  str_drop(t);
  return ok;
}

_Bool strata_parse_float(Str *s, double *out) {
  Str *t = strata_str_trim(s);
  _Bool ok = parse_float_trimmed(t, out);
  str_drop(t);
  return ok;
}

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

// A float with `p` decimals, as Rust's `{:.p}`.
Str *strata_show_float_fixed(double x, int64_t p) {
  int n = snprintf(NULL, 0, "%.*f", (int)p, x);
  Str *s = str_alloc(n);
  char *tmp = malloc((size_t)n + 1);
  snprintf(tmp, (size_t)n + 1, "%.*f", (int)p, x);
  memcpy(s->bytes, tmp, (size_t)n);
  free(tmp);
  return s;
}

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
