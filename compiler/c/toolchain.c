// The toolchain built into the compiler: LLVM IR (compiler/codegen's text)
// to an object file, and object files to an executable, in this process, with
// the LLVM and lld linked into the compiler's executable. No C compiler or
// linker has to be installed where `strata build` runs.
//
// LLVM's C API; lld, which has no C API, through c/lld.cpp (the only C++).
// Called from Strata through `extern c` (compiler/toolchain).

#include <llvm-c/Core.h>
#include <llvm-c/IRReader.h>
#include <llvm-c/Target.h>
#include <llvm-c/TargetMachine.h>
#include <llvm-c/Transforms/PassBuilder.h>

#include <CommonCrypto/CommonDigest.h>
#include <copyfile.h>
#include <limits.h>
#include <mach-o/dyld.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/clonefile.h>
#include <sys/stat.h>
#include <unistd.h>

// ---- growing buffers ----

typedef struct {
  char *p;
  size_t n, cap;
} Buf;

static void buf_add(Buf *b, const char *s, size_t n) {
  if (b->n + n + 1 > b->cap) {
    size_t c = b->cap ? b->cap : 256;
    while (b->n + n + 1 > c) c *= 2;
    b->p = realloc(b->p, c);
    b->cap = c;
  }
  memcpy(b->p + b->n, s, n);
  b->n += n;
  b->p[b->n] = 0;
}

static void buf_str(Buf *b, const char *s) { buf_add(b, s, strlen(s)); }

static void buf_int(Buf *b, long long v) {
  char t[32];
  buf_add(b, t, (size_t)snprintf(t, sizeof t, "%lld", v));
}

static void buf_clear(Buf *b) {
  b->n = 0;
  if (b->p) b->p[0] = 0;
}

// (never NULL: "" when nothing was added)
static const char *buf_text(Buf *b) { return b->p ? b->p : ""; }

typedef struct {
  int *p;
  size_t n, cap;
} Ints;

static void ints_push(Ints *v, int x) {
  if (v->n == v->cap) {
    v->cap = v->cap ? v->cap * 2 : 8;
    v->p = realloc(v->p, v->cap * sizeof(int));
  }
  v->p[v->n++] = x;
}

static Buf last_error;

static int32_t fail(const char *m) {
  buf_clear(&last_error);
  buf_str(&last_error, m);
  return 1;
}

// Profiling (c/jit.c): every function defined keeps its frame pointer.
void toolchain_frame_pointers(LLVMModuleRef mod);
int32_t toolchain_profiling(void);

// SelectionDAG's instruction selection, FastISel at -O0, on a target
// machine (c/jit.c).
void toolchain_selection(LLVMTargetMachineRef tm);

// The IR text as a module, in a context of its own (c/ir.c): 0, else 1
// and why.
int32_t toolchain_module(const char *ir, LLVMContextRef *ctx, LLVMModuleRef *mod, char **msg);

// `--timings` (c/driver.c): where a phase begins and ends.
int32_t timings_begin(const char *name);
int32_t timings_end(const char *name);

// lld's Mach-O linker (c/lld.cpp): 1 when it linked.
int toolchain_lld(int argc, const char **argv);

// Why the last `toolchain_object` or `toolchain_link` failed ("" when it didn't).
const char *toolchain_error(void) { return buf_text(&last_error); }

// What objects are made for: Apple silicon Macs from macOS 13 on, any of
// them (the CPU clang picks for this target), whichever Mac builds them, so
// that an executable, and the precompiled pieces a release ships, run on
// every one. `toolchain_link` passes the same version to lld.
static const char *TRIPLE = "arm64-apple-macosx13.0.0";
static const char *CPU = "apple-m1";

const char *toolchain_triple(void) { return TRIPLE; }

// What an object `toolchain_object` makes depends on besides its IR and
// optimization level: LLVM's version and what it compiles for. Part of the
// keys of the objects the caches keep by their IR (std's library, core.rt),
// so a compiler with another LLVM or target makes its own. Change it with
// anything here that changes the code made from the same IR.
const char *toolchain_settings(void) {
  static char s[160];
  unsigned major, minor, patch;
  LLVMGetVersion(&major, &minor, &patch);
  snprintf(s, sizeof s, "LLVM %u.%u.%u %s %s PIC SelectionDAG", major, minor, patch, TRIPLE, CPU);
  return s;
}

static LLVMCodeGenOptLevel level_of(int32_t opt) {
  return opt <= 0 ? LLVMCodeGenLevelNone
         : opt == 1 ? LLVMCodeGenLevelLess
         : opt == 2 ? LLVMCodeGenLevelDefault
                    : LLVMCodeGenLevelAggressive;
}

static void init_target(void) {
  LLVMInitializeAArch64TargetInfo();
  LLVMInitializeAArch64Target();
  LLVMInitializeAArch64TargetMC();
  LLVMInitializeAArch64AsmPrinter();
  LLVMInitializeAArch64AsmParser();
}

// "A: B" (B may be NULL) in `b`.
static void say(Buf *b, const char *a, const char *x, const char *c) {
  buf_clear(b);
  buf_str(b, a);
  if (x) buf_str(b, x);
  if (c) buf_str(b, c);
}

// Compile IR text to an object file at `out`, optimized at `opt` (0-3: the
// passes of -O<opt>, then code generated at that level). 0 when it is
// written, else 1 (`toolchain_error` says why).
int32_t toolchain_object(const char *ir, const char *out, int32_t opt) {
  buf_clear(&last_error);
  init_target();
  LLVMContextRef ctx;
  LLVMModuleRef mod;
  char *msg = NULL;
  if (toolchain_module(ir, &ctx, &mod, &msg)) {
    say(&last_error, "LLVM rejected the generated IR: ", msg ? msg : "", NULL);
    LLVMDisposeMessage(msg);
    return 1;
  }
  const char *triple = TRIPLE;
  LLVMTargetRef target;
  if (LLVMGetTargetFromTriple(triple, &target, &msg)) {
    say(&last_error, "no target for ", triple, ": ");
    buf_str(&last_error, msg ? msg : "");
    LLVMDisposeMessage(msg);
    LLVMDisposeModule(mod);
    LLVMContextDispose(ctx);
    return 1;
  }
  LLVMSetTarget(mod, triple);
  toolchain_frame_pointers(mod);
  // position-independent, as every macOS executable
  LLVMTargetMachineRef tm =
      LLVMCreateTargetMachine(target, triple, CPU, "", level_of(opt), LLVMRelocPIC, LLVMCodeModelDefault);
  toolchain_selection(tm);
  char *layout = LLVMCopyStringRepOfTargetData(LLVMCreateTargetDataLayout(tm));
  LLVMSetDataLayout(mod, layout);
  LLVMDisposeMessage(layout);
  int32_t r = 0;
  if (opt > 0) {
    char passes[32];
    snprintf(passes, sizeof passes, "default<O%d>", opt > 3 ? 3 : opt);
    LLVMPassBuilderOptionsRef po = LLVMCreatePassBuilderOptions();
    timings_begin("LLVM: optimize");
    LLVMErrorRef e = LLVMRunPasses(mod, passes, tm, po);
    timings_end("LLVM: optimize");
    LLVMDisposePassBuilderOptions(po);
    if (e) {
      char *m = LLVMGetErrorMessage(e);
      say(&last_error, "LLVM's optimizer failed: ", m, NULL);
      r = 1;
      LLVMDisposeErrorMessage(m);
    }
  }
  timings_begin("LLVM: machine code");
  if (!r && LLVMTargetMachineEmitToFile(tm, mod, out, LLVMObjectFile, &msg)) {
    say(&last_error, "can't write ", out, ": ");
    buf_str(&last_error, msg ? msg : "");
    r = 1;
    LLVMDisposeMessage(msg);
  }
  timings_end("LLVM: machine code");
  LLVMDisposeTargetMachine(tm);
  LLVMDisposeModule(mod);
  LLVMContextDispose(ctx);
  return r;
}

// ---- The IR in parts, compiled in parallel, each part's object cached ----
//
// `toolchain_objects` cuts the program's IR into parts (runs of whole
// function definitions), each made a module of its own: its functions, the
// private constants and internal functions they reach (copied, renamed in
// the order the part first names them, so that a part's text doesn't depend
// on how the rest of the program numbers its constants), and a declaration
// of each other function it calls. The parts are compiled by worker threads,
// each with its own LLVM context and target machine; a part's object is kept
// in the cache folder under the SHA-256 of its text (and of what it is
// compiled for), so that a part whose functions didn't change is not
// compiled again. The objects are listed in the parts' order, whatever order
// the threads finish in: the same IR always makes the same objects.

enum { COMMON, GLOBAL, DECLARE, DEFINE };

// Where an item names a local item: [from, to) in its text, the `@`
// included, and that item.
typedef struct {
  size_t from, to;
  int item;
} Span;

// One top-level entity of the IR.
typedef struct {
  int kind;
  const char *text; // its lines, the last newline included
  size_t len;
  const char *name; // a global's, declaration's or function's (`@` left out, quotes kept)
  size_t name_len;
  int local; // private or internal: copied into each part that reaches it
  Ints refs; // the items it names (indices into the items)
  Span *spans;
  size_t nspans, capspans;
} Item;

static int name_char(char c) {
  return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '-' ||
         c == '$' || c == '.' || c == '_';
}

// The length of the name at `p` of `t` (`n` bytes; just after an `@`): a
// quoted name with its quotes, else the run of name characters.
static size_t name_at(const char *t, size_t n, size_t p) {
  if (p < n && t[p] == '"') {
    const char *q = memchr(t + p + 1, '"', n - p - 1);
    size_t e = q ? (size_t)(q - t) : n - 1;
    return e + 1 - p;
  }
  size_t q = p;
  while (q < n && name_char(t[q])) q++;
  return q - p;
}

// The first `c` in t[from, n), or `n`.
static size_t find(const char *t, size_t n, size_t from, char c) {
  if (from >= n) return n;
  const char *q = memchr(t + from, c, n - from);
  return q ? (size_t)(q - t) : n;
}

// ---- names to items: open addressing over (bytes, length) ----

typedef struct {
  int *slot; // item index + 1, 0 when empty
  size_t mask;
  Item *items;
} Names;

static uint64_t fnv(const char *s, size_t n) {
  uint64_t h = 14695981039346656037ull;
  for (size_t i = 0; i < n; i++) h = (h ^ (unsigned char)s[i]) * 1099511628211ull;
  return h;
}

// The slot for the name `s` (found, or where it would go).
static int *names_slot(Names *m, const char *s, size_t n) {
  size_t i = fnv(s, n) & m->mask;
  for (;;) {
    int k = m->slot[i];
    if (!k) return &m->slot[i];
    Item *it = &m->items[k - 1];
    if (it->name_len == n && memcmp(it->name, s, n) == 0) return &m->slot[i];
    i = (i + 1) & m->mask;
  }
}

// The item named `s`, or -1.
static int names_get(Names *m, const char *s, size_t n) { return *names_slot(m, s, n) - 1; }

// The declaration of the function whose `define` line is `h` (`n` bytes):
// its return type, name and parameters' types, without their names or the
// function's attributes.
static void declaration_of(Buf *out, const char *h, size_t n) {
  // `define RET @NAME(TYPE %a, TYPE %b) ATTRS {`
  size_t at = find(h, n, 0, '@');
  size_t open = at + 1 + name_at(h, n, at + 1); // the `(`
  buf_str(out, "declare ");
  buf_add(out, h + 7, open - 7);
  int depth = 0;
  size_t end = open;
  for (; end < n; end++) {
    char c = h[end];
    if (c == '(' || c == '[' || c == '{' || c == '<') depth++;
    if (c == ')' || c == ']' || c == '}' || c == '>') depth--;
    if (depth == 0) break;
  }
  // the parameters, h[open, end), each ` %name` dropped
  const char *ps = h + open;
  size_t pn = end - open;
  for (size_t k = 0; k < pn; k++) {
    if (ps[k] == ' ' && k + 1 < pn && ps[k + 1] == '%') {
      k++;
      if (k + 1 < pn && ps[k + 1] == '"') {
        const char *q = memchr(ps + k + 2, '"', k + 2 < pn ? pn - k - 2 : 0);
        k = q ? (size_t)(q - ps) : pn;
      } else {
        while (k + 1 < pn && name_char(ps[k + 1])) k++;
      }
      continue;
    }
    buf_add(out, ps + k, 1);
  }
  buf_str(out, ")\n");
}

static int starts(const char *t, size_t n, const char *p) {
  size_t m = strlen(p);
  return n >= m && memcmp(t, p, m) == 0;
}

// `@.lN` in `s`.
static void local_name(Buf *s, int n) {
  buf_str(s, "@.l");
  buf_int(s, n);
}

// The item `k` in `s`, the local items it names (itself too, when local)
// renamed by `number`.
static void emit_item(Buf *s, Item *items, const int *number, int k) {
  Item *it = &items[k];
  size_t last = 0;
  if (it->local) {
    // its own name, renamed too
    size_t at = it->kind == GLOBAL ? 0 : find(it->text, it->len, 0, '@');
    size_t n = name_at(it->text, it->len, at + 1);
    buf_add(s, it->text, at);
    local_name(s, number[k]);
    last = at + 1 + n;
  }
  for (size_t j = 0; j < it->nspans; j++) {
    Span sp = it->spans[j];
    buf_add(s, it->text + last, sp.from - last);
    local_name(s, number[sp.item]);
    last = sp.to;
  }
  buf_add(s, it->text + last, it->len - last);
}

static int cmp_int(const void *a, const void *b) {
  int x = *(const int *)a, y = *(const int *)b;
  return x < y ? -1 : x > y;
}

// `ir` read into its items and its functions cut into parts' texts. A part
// ends after a function once it holds at least `min_bytes`, at a function
// whose name hashes to 0 modulo 8, or at `max_bytes`: where parts end
// depends only on the functions around, not on the rest of the program, so
// an edit changes the parts of what it changed. A function of another part
// of at most `import_bytes` that a part's functions call is copied into it
// `available_externally` (to inline, as ThinLTO imports them; compiled only
// in its own part). 0 when the IR has what can't be split (a global defined
// for other modules to use); else 1, the parts in `*out` (`*count` of them).
static int split_ir(const char *ir, size_t size, size_t min_bytes, size_t max_bytes, size_t import_bytes,
                    Buf **out, size_t *count) {
  Item *items = NULL;
  size_t n_items = 0, cap_items = 0;
  int ok = 1;
  size_t i = 0;
  while (i < size) {
    size_t e = find(ir, size, i, '\n');
    e = e == size ? size : e + 1;
    const char *line = ir + i;
    size_t ll = e - i;
    Item it;
    memset(&it, 0, sizeof it);
    it.kind = COMMON;
    it.text = line;
    it.len = ll;
    if (starts(line, ll, "define ")) {
      // to the line `}`
      size_t j = e;
      while (j < size) {
        size_t k = find(ir, size, j, '\n');
        k = k == size ? size : k + 1;
        const char *l = ir + j;
        size_t lk = k - j;
        j = k;
        if ((lk == 2 && l[0] == '}' && l[1] == '\n') || (lk == 1 && l[0] == '}')) break;
      }
      e = j;
      it.kind = DEFINE;
      it.text = ir + i;
      it.len = e - i;
      size_t at = find(line, ll, 0, '@');
      it.name = line + at + 1;
      it.name_len = name_at(line, ll, at + 1);
      it.local = starts(line, ll, "define internal ") || starts(line, ll, "define private ");
    } else if (starts(line, ll, "declare ")) {
      it.kind = DECLARE;
      size_t at = find(line, ll, 0, '@');
      it.name = line + at + 1;
      it.name_len = name_at(line, ll, at + 1);
    } else if (starts(line, ll, "@")) {
      it.kind = GLOBAL;
      size_t n = name_at(line, ll, 1);
      it.name = line + 1;
      it.name_len = n;
      const char *rest = line + 1 + n;
      size_t rl = ll - 1 - n;
      it.local = starts(rest, rl, " = private ") || starts(rest, rl, " = internal ");
      if (!it.local && !starts(rest, rl, " = external ")) {
        ok = 0;
        break;
      }
    } else if (starts(line, ll, ";") || (ll == 1 && line[0] == '\n')) {
      // comments (counts that change with the program) and blank lines
      i = e;
      continue;
    }
    if (n_items == cap_items) {
      cap_items = cap_items ? cap_items * 2 : 1024;
      items = realloc(items, cap_items * sizeof(Item));
    }
    items[n_items++] = it;
    i = e;
  }
  if (!ok) {
    free(items);
    return 0;
  }
  // names to items: a definition before a declaration
  Names names;
  size_t slots = 16;
  while (slots < n_items * 2) slots *= 2;
  names.slot = calloc(slots, sizeof(int));
  names.mask = slots - 1;
  names.items = items;
  for (size_t k = 0; k < n_items; k++) {
    Item *it = &items[k];
    if (it->kind == COMMON) continue;
    int *s = names_slot(&names, it->name, it->name_len);
    if (!*s || (items[*s - 1].kind == DECLARE && it->kind != DECLARE)) *s = (int)k + 1;
  }
  for (size_t k = 0; k < n_items; k++) {
    Item *it = &items[k];
    if (it->kind != DEFINE && it->kind != GLOBAL) continue;
    // (its own name is not a reference)
    size_t skip = it->kind == GLOBAL ? 1 + it->name_len : find(it->text, it->len, 0, '@') + 1 + it->name_len;
    // each `@name` in its text (outside string constants and comments)
    const char *t = it->text;
    size_t n = it->len, p = 0;
    while (p < n) {
      char c = t[p];
      if (c == ';') {
        while (p < n && t[p] != '\n') p++;
      } else if (c == '"') {
        // a string constant (`c"..."`, `!"..."`): its quotes are escaped (\22)
        size_t q = find(t, n, p + 1, '"');
        p = q == n ? n : q + 1;
      } else if (c == '%' && p + 1 < n && t[p + 1] == '"') {
        size_t q = find(t, n, p + 2, '"');
        p = q == n ? n : q + 1;
      } else if (c == '@') {
        size_t m = name_at(t, n, p + 1);
        if (m && p >= skip) {
          int r = names_get(&names, t + p + 1, m);
          if (r >= 0) {
            ints_push(&it->refs, r);
            if (items[r].local) {
              if (it->nspans == it->capspans) {
                it->capspans = it->capspans ? it->capspans * 2 : 8;
                it->spans = realloc(it->spans, it->capspans * sizeof(Span));
              }
              it->spans[it->nspans++] = (Span){p, p + 1 + m, r};
            }
          }
        }
        p += 1 + m;
      } else {
        p++;
      }
    }
  }
  // the groups: runs of the functions (not local) in order, each ended by a
  // -1 in `groups`
  Ints groups = {0};
  size_t n_groups = 0, bytes = 0;
  int open = 0;
  for (size_t k = 0; k < n_items; k++) {
    Item *it = &items[k];
    if (it->kind != DEFINE || it->local) continue;
    ints_push(&groups, (int)k);
    open = 1;
    bytes += it->len;
    uint64_t h = fnv(it->name, it->name_len);
    if ((bytes >= min_bytes && (h & 7) == 0) || bytes >= max_bytes) {
      ints_push(&groups, -1);
      n_groups++;
      open = 0;
      bytes = 0;
    }
  }
  if (open) {
    ints_push(&groups, -1);
    n_groups++;
  }
  Buf common = {0};
  for (size_t k = 0; k < n_items; k++)
    if (items[k].kind == COMMON) buf_add(&common, items[k].text, items[k].len);
  char *seen = calloc(n_items + 1, 1), *own = calloc(n_items + 1, 1);
  int *number = malloc((n_items + 1) * sizeof(int));
  for (size_t k = 0; k < n_items; k++) number[k] = -1;
  Buf *parts = calloc(n_groups ? n_groups : 1, sizeof(Buf));
  size_t g0 = 0;
  for (size_t gi = 0; gi < n_groups; gi++) {
    size_t g1 = g0;
    while (groups.p[g1] != -1) g1++;
    // its functions, then the local items they reach (transitively), in the
    // order first named, and the small functions they call; the other names
    // declared
    Ints order = {0}, outside = {0}, imported = {0}, queue = {0};
    for (size_t x = g0; x < g1; x++) {
      int k = groups.p[x];
      ints_push(&queue, k);
      seen[k] = own[k] = 1;
    }
    for (size_t q = 0; q < queue.n; q++) {
      int may_import = import_bytes > 0 && own[queue.p[q]];
      Ints *refs = &items[queue.p[q]].refs;
      for (size_t x = 0; x < refs->n; x++) {
        int r = refs->p[x];
        if (seen[r]) continue;
        seen[r] = 1;
        Item *it = &items[r];
        if (it->local) {
          number[r] = (int)order.n;
          ints_push(&order, r);
          ints_push(&queue, r);
        } else if (may_import && it->kind == DEFINE && it->len <= import_bytes &&
                   !(it->name_len == 4 && memcmp(it->name, "main", 4) == 0)) {
          ints_push(&imported, r);
          ints_push(&queue, r);
        } else {
          ints_push(&outside, r);
        }
      }
    }
    Buf *s = &parts[gi];
    buf_add(s, buf_text(&common), common.n);
    if (outside.n) qsort(outside.p, outside.n, sizeof(int), cmp_int);
    for (size_t x = 0; x < outside.n; x++) {
      Item *it = &items[outside.p[x]];
      if (it->kind == DEFINE)
        declaration_of(s, it->text, find(it->text, it->len, 0, '\n'));
      else
        buf_add(s, it->text, it->len); // a declaration, or an external global
    }
    for (size_t x = 0; x < order.n; x++) emit_item(s, items, number, order.p[x]);
    for (size_t x = 0; x < imported.n; x++) {
      buf_str(s, "define available_externally ");
      size_t at = s->n;
      emit_item(s, items, number, imported.p[x]);
      // its `define `
      memmove(s->p + at, s->p + at + 7, s->n - at - 7 + 1);
      s->n -= 7;
    }
    for (size_t x = g0; x < g1; x++) emit_item(s, items, number, groups.p[x]);
    for (size_t x = g0; x < g1; x++) own[groups.p[x]] = 0;
    for (size_t x = 0; x < queue.n; x++) seen[queue.p[x]] = 0;
    for (size_t x = 0; x < outside.n; x++) seen[outside.p[x]] = 0;
    for (size_t x = 0; x < order.n; x++) number[order.p[x]] = -1;
    free(order.p);
    free(outside.p);
    free(imported.p);
    free(queue.p);
    g0 = g1 + 1;
  }
  for (size_t k = 0; k < n_items; k++) {
    free(items[k].refs.p);
    free(items[k].spans);
  }
  free(items);
  free(names.slot);
  free(groups.p);
  free(common.p);
  free(seen);
  free(own);
  free(number);
  *out = parts;
  *count = n_groups;
  return 1;
}

// The SHA-256 of `prefix` and `text` as 64 hex digits (and a 0) in `hex`.
static void hex_sha256(char hex[65], const char *prefix, size_t pn, const char *text, size_t tn) {
  CC_SHA256_CTX c;
  CC_SHA256_Init(&c);
  CC_SHA256_Update(&c, prefix, (CC_LONG)pn);
  CC_SHA256_Update(&c, text, (CC_LONG)tn);
  unsigned char d[32];
  CC_SHA256_Final(d, &c);
  static const char *hx = "0123456789abcdef";
  for (int k = 0; k < 32; k++) {
    hex[2 * k] = hx[d[k] >> 4];
    hex[2 * k + 1] = hx[d[k] & 15];
  }
  hex[64] = 0;
}

// One part compiled to `out`, as `toolchain_object` compiles, in a context
// and with a target machine of its own: NULL when written, else why not
// (malloc'd).
static char *compile_part(const char *ir, size_t size, const char *out, int32_t opt) {
  Buf r = {0};
  LLVMContextRef ctx = LLVMContextCreate();
  LLVMMemoryBufferRef mb = LLVMCreateMemoryBufferWithMemoryRange(ir, size, "strata", 1);
  LLVMModuleRef mod;
  char *msg = NULL;
  if (LLVMParseIRInContext(ctx, mb, &mod, &msg)) {
    say(&r, "LLVM rejected the generated IR: ", msg ? msg : "", NULL);
    LLVMDisposeMessage(msg);
    LLVMContextDispose(ctx);
    return r.p;
  }
  LLVMTargetRef target;
  if (LLVMGetTargetFromTriple(TRIPLE, &target, &msg)) {
    say(&r, "no target for ", TRIPLE, ": ");
    buf_str(&r, msg ? msg : "");
    LLVMDisposeMessage(msg);
    LLVMDisposeModule(mod);
    LLVMContextDispose(ctx);
    return r.p;
  }
  LLVMSetTarget(mod, TRIPLE);
  toolchain_frame_pointers(mod);
  LLVMTargetMachineRef tm =
      LLVMCreateTargetMachine(target, TRIPLE, CPU, "", level_of(opt), LLVMRelocPIC, LLVMCodeModelDefault);
  toolchain_selection(tm);
  LLVMTargetDataRef td = LLVMCreateTargetDataLayout(tm);
  char *layout = LLVMCopyStringRepOfTargetData(td);
  LLVMSetDataLayout(mod, layout);
  LLVMDisposeMessage(layout);
  LLVMDisposeTargetData(td);
  if (opt > 0) {
    char passes[32];
    snprintf(passes, sizeof passes, "default<O%d>", opt > 3 ? 3 : opt);
    LLVMPassBuilderOptionsRef po = LLVMCreatePassBuilderOptions();
    LLVMErrorRef e = LLVMRunPasses(mod, passes, tm, po);
    LLVMDisposePassBuilderOptions(po);
    if (e) {
      char *m = LLVMGetErrorMessage(e);
      say(&r, "LLVM's optimizer failed: ", m, NULL);
      LLVMDisposeErrorMessage(m);
    }
  }
  if (!r.n && LLVMTargetMachineEmitToFile(tm, mod, out, LLVMObjectFile, &msg)) {
    say(&r, "can't write ", out, ": ");
    buf_str(&r, msg ? msg : "");
    LLVMDisposeMessage(msg);
  }
  LLVMDisposeTargetMachine(tm);
  LLVMDisposeModule(mod);
  LLVMContextDispose(ctx);
  return r.n ? r.p : (free(r.p), NULL);
}

// (the largest function a part copies from another to inline: its text,
// about 30 lines; `STRATA_IMPORT_BYTES` to try others)
static size_t import_bytes(void) {
  const char *e = getenv("STRATA_IMPORT_BYTES");
  return e ? strtoul(e, 0, 10) : 2048;
}

static Buf objects_list;
static int64_t objects_compiled = 0;

static void make_dirs(const char *p) {
  size_t n = strlen(p);
  char *t = malloc(n + 1);
  memcpy(t, p, n + 1);
  for (size_t k = 1; k < n; k++)
    if (t[k] == '/') {
      t[k] = 0;
      mkdir(t, 0755);
      t[k] = '/';
    }
  mkdir(t, 0755);
  free(t);
}

// The objects the last `toolchain_objects` made, in order, each after a
// newline.
const char *toolchain_objects_list(void) { return buf_text(&objects_list); }

// `s` framed by its length: `N:` then the bytes, hashed into `c`.
static void sha_framed(CC_SHA256_CTX *c, const char *s, size_t n) {
  char t[32];
  int k = snprintf(t, sizeof t, "%zu:", n);
  CC_SHA256_Update(c, t, (CC_LONG)k);
  CC_SHA256_Update(c, s, (CC_LONG)n);
}

static void sha_hex(CC_SHA256_CTX *c, char hex[65]) {
  unsigned char d[32];
  CC_SHA256_Final(d, c);
  static const char *hx = "0123456789abcdef";
  for (int k = 0; k < 32; k++) {
    hex[2 * k] = hx[d[k] >> 4];
    hex[2 * k + 1] = hx[d[k] & 15];
  }
  hex[64] = 0;
}

// The SHA-256, as 64 hex digits, of `what` and of the bytes of the files
// `paths` (each after a newline), each framed by its length (a missing file
// counts as `-1`): the key of what is made of those files.
const char *toolchain_files_key(const char *what, const char *paths) {
  static char key[65];
  CC_SHA256_CTX c;
  CC_SHA256_Init(&c);
  sha_framed(&c, what, strlen(what));
  Buf p = {0}, bytes = {0};
  const char *s = paths;
  while (*s) {
    const char *e = strchr(s, '\n');
    size_t n = e ? (size_t)(e - s) : strlen(s);
    buf_clear(&p);
    buf_add(&p, s, n);
    s += n + (e ? 1 : 0);
    if (!n) continue;
    FILE *f = fopen(p.p, "rb");
    if (!f) {
      CC_SHA256_Update(&c, "-1:", 3);
      continue;
    }
    buf_clear(&bytes);
    char b[65536];
    size_t k;
    while ((k = fread(b, 1, sizeof b, f)) > 0) buf_add(&bytes, b, k);
    fclose(f);
    sha_framed(&c, buf_text(&bytes), bytes.n);
  }
  free(p.p);
  free(bytes.p);
  sha_hex(&c, key);
  return key;
}

// The SHA-256 (64 hex digits) of a link's arguments `args` (each after a
// newline), with each one's size and time of change that names a file or a
// folder (`-LDIR` too, for the libraries `-lNAME` finds there): what a
// linked executable is kept in the cache under. An object of the cache is
// named by its contents already; a library outside it (LLVM's) is told by
// its size and time.
const char *toolchain_link_key(const char *args) {
  static char key[65];
  CC_SHA256_CTX c;
  CC_SHA256_Init(&c);
  CC_SHA256_Update(&c, "strata link 1\n", 14);
  Buf a = {0};
  const char *s = args;
  size_t total = strlen(args), i = 0;
  while (i < total) {
    const char *e = memchr(s + i, '\n', total - i);
    size_t n = e ? (size_t)(e - (s + i)) : total - i;
    buf_clear(&a);
    buf_add(&a, s + i, n);
    i += n + 1;
    sha_framed(&c, buf_text(&a), n);
    const char *path = n >= 2 && a.p[0] == '-' && a.p[1] == 'L' ? a.p + 2 : buf_text(&a);
    struct stat st;
    if (path[0] == '/' && stat(path, &st) == 0) {
      char t[96];
      int k = snprintf(t, sizeof t, " %lld %lld.%lld", (long long)st.st_size, (long long)st.st_mtimespec.tv_sec,
                       (long long)st.st_mtimespec.tv_nsec);
      CC_SHA256_Update(&c, t, (CC_LONG)k);
    }
    CC_SHA256_Update(&c, "\n", 1);
  }
  free(a.p);
  sha_hex(&c, key);
  return key;
}

// The file `from` copied to `to` (replaced): a clone where the file system
// makes them (APFS: no bytes copied), else copied. 0 when done.
int32_t toolchain_clone(const char *from, const char *to) {
  unlink(to);
  if (clonefile(from, to, 0) == 0) return 0;
  return copyfile(from, to, NULL, COPYFILE_ALL) == 0 ? 0 : 1;
}

// How many parts the last `toolchain_objects` compiled (the others' objects
// were in the cache).
int64_t toolchain_objects_compiled(void) { return objects_compiled; }

// The parts' work, shared by the threads.
typedef struct {
  Buf *parts;
  char **outs, **errs;
  int *work;
  size_t n_work;
  int cached; // the outputs are the cache's: written beside, then renamed
  int32_t opt;
  _Atomic size_t next;
} Work;

static void *worker(void *arg) {
  Work *w = arg;
  for (;;) {
    size_t x = atomic_fetch_add(&w->next, 1);
    if (x >= w->n_work) return NULL;
    int k = w->work[x];
    Buf *part = &w->parts[k];
    if (!w->cached) {
      w->errs[k] = compile_part(buf_text(part), part->n, w->outs[k], w->opt);
    } else {
      // written beside it, then renamed: a part is in the cache whole or not at all
      const char *out = w->outs[k];
      char dir[PATH_MAX];
      snprintf(dir, sizeof dir, "%s", out);
      char *slash = strrchr(dir, '/');
      if (slash) *slash = 0;
      mkdir(dir, 0755);
      char tmp[PATH_MAX + 64];
      snprintf(tmp, sizeof tmp, "%s.tmp-%d-%d", out, (int)getpid(), k);
      w->errs[k] = compile_part(buf_text(part), part->n, tmp, w->opt);
      if (!w->errs[k] && rename(tmp, out) != 0) {
        Buf e = {0};
        say(&e, "can't write ", out, NULL);
        w->errs[k] = e.p;
      }
      if (w->errs[k]) unlink(tmp);
    }
  }
}

// Largest parts first (ties in the parts' order).
static Buf *sort_parts;
static int by_size(const void *a, const void *b) {
  int x = *(const int *)a, y = *(const int *)b;
  size_t sx = sort_parts[x].n, sy = sort_parts[y].n;
  if (sx != sy) return sx > sy ? -1 : 1;
  return x < y ? -1 : x > y;
}

// The IR `ir` compiled, optimized at `opt` (0-3), to objects: in parts, by
// `threads` threads (0: one a core), each part's object kept in the folder
// `cache` under its key, or (`cache` "") written as `scratch`/part.N.o.
// How many objects (`toolchain_objects_list` names them), or -1
// (`toolchain_error` says why).
int32_t toolchain_objects(const char *ir, const char *scratch, const char *cache, int32_t opt,
                          int32_t threads) {
  buf_clear(&last_error);
  buf_clear(&objects_list);
  objects_compiled = 0;
  init_target();
  size_t size = strlen(ir);
  const char *whole = getenv("STRATA_LLVM_PARTS");
  int one = whole && strcmp(whole, "0") == 0;
  unsigned major = 0, minor = 0, patch = 0;
  LLVMGetVersion(&major, &minor, &patch);
  char what[160];
  // (`--profile` keeps frame pointers: other objects)
  int what_n = snprintf(what, sizeof what, "strata object 2\n%s %s O%d LLVM %u.%u.%u%s\n", TRIPLE, CPU, (int)opt,
                        major, minor, patch, toolchain_profiling() ? " profiling" : "");
  const char *dir = cache;
  int cached = dir[0] != 0;
  // the objects of an IR made before, listed under the key of the whole
  // text (`lists/KEY`): neither cut into parts nor each part's key worked
  // out again, when every one is still there
  Buf list = {0};
  if (cached) {
    Buf prefix = {0};
    buf_add(&prefix, what, (size_t)what_n);
    buf_str(&prefix, one ? "list whole\n" : "list\n");
    char key[65];
    hex_sha256(key, prefix.p, prefix.n, ir, size);
    free(prefix.p);
    buf_str(&list, dir);
    buf_str(&list, "/lists/");
    buf_str(&list, key);
    FILE *f = fopen(list.p, "r");
    if (f) {
      Buf got = {0};
      char b[4096];
      size_t n;
      while ((n = fread(b, 1, sizeof b, f)) > 0) buf_add(&got, b, n);
      fclose(f);
      int32_t count = 0;
      int all = got.n > 0;
      size_t at = 0;
      while (all && at < got.n) {
        size_t e = find(got.p, got.n, at, '\n');
        char path[PATH_MAX];
        snprintf(path, sizeof path, "%.*s", (int)(e - at), got.p + at);
        struct stat st;
        if (stat(path, &st) != 0) all = 0;
        count++;
        at = e + 1;
      }
      if (all) {
        buf_add(&objects_list, got.p, got.n);
        free(got.p);
        free(list.p);
        return count;
      }
      free(got.p);
    }
  }
  // (optimized, larger parts: more of what a function calls is beside it,
  // to inline; `STRATA_LLVM_PARTS=0`: the whole module as one part, to
  // compare)
  timings_begin("LLVM: split IR");
  Buf *parts = NULL;
  size_t n_parts = 0;
  if (one || !split_ir(ir, size, opt > 0 ? 512 * 1024 : 128 * 1024, 4 * 1024 * 1024, opt > 0 ? import_bytes() : 0,
                       &parts, &n_parts)) {
    parts = calloc(1, sizeof(Buf));
    buf_add(&parts[0], ir, size);
    n_parts = 1;
  }
  timings_end("LLVM: split IR");
  if (cached) make_dirs(dir);
  char **outs = calloc(n_parts + 1, sizeof(char *)), **errs = calloc(n_parts + 1, sizeof(char *));
  int *work = malloc((n_parts + 1) * sizeof(int));
  size_t n_work = 0;
  for (size_t k = 0; k < n_parts; k++) {
    Buf o = {0};
    if (!cached) {
      buf_str(&o, scratch);
      buf_str(&o, "/part.");
      buf_int(&o, (long long)k);
      buf_str(&o, ".o");
      work[n_work++] = (int)k;
    } else {
      char key[65];
      hex_sha256(key, what, (size_t)what_n, buf_text(&parts[k]), parts[k].n);
      buf_str(&o, dir);
      buf_str(&o, "/");
      buf_add(&o, key, 2);
      buf_str(&o, "/");
      buf_str(&o, key);
      buf_str(&o, ".o");
      struct stat st;
      if (stat(o.p, &st) != 0) work[n_work++] = (int)k;
    }
    outs[k] = o.p;
  }
  objects_compiled = (int64_t)n_work;
  // the largest parts first, so that no thread is left with a long one at the end
  sort_parts = parts;
  if (n_work) qsort(work, n_work, sizeof(int), by_size);
  long cores = sysconf(_SC_NPROCESSORS_ONLN);
  size_t n = threads > 0 ? (size_t)threads : cores > 0 ? (size_t)cores : 1;
  if (n > n_work) n = n_work;
  timings_begin("LLVM: parts (parallel)");
  Work w = {parts, outs, errs, work, n_work, cached, opt, 0};
  pthread_t *pool = calloc(n + 1, sizeof(pthread_t));
  size_t started = 0;
  for (size_t t = 1; t < n; t++) {
    // (the main thread's stack, not a secondary thread's 512 KB: deep IR
    // makes deep recursion in LLVM)
    pthread_attr_t attr;
    pthread_attr_init(&attr);
    pthread_attr_setstacksize(&attr, 8 << 20);
    if (pthread_create(&pool[started], &attr, worker, &w) == 0) started++;
    pthread_attr_destroy(&attr);
  }
  if (n > 0) worker(&w);
  for (size_t t = 0; t < started; t++) pthread_join(pool[t], NULL);
  free(pool);
  timings_end("LLVM: parts (parallel)");
  int32_t r = (int32_t)n_parts;
  for (size_t k = 0; k < n_parts; k++)
    if (errs[k]) {
      fail(errs[k]);
      r = -1;
      break;
    }
  if (r >= 0) {
    for (size_t k = 0; k < n_parts; k++) {
      buf_str(&objects_list, outs[k]);
      buf_str(&objects_list, "\n");
    }
    if (list.n) {
      // (written beside it, then renamed: whole or not at all)
      Buf lists = {0}, tmp = {0};
      buf_str(&lists, dir);
      buf_str(&lists, "/lists");
      make_dirs(lists.p);
      buf_str(&tmp, list.p);
      buf_str(&tmp, ".tmp-");
      buf_int(&tmp, (long long)getpid());
      FILE *f = fopen(tmp.p, "w");
      if (f) {
        int ok = fwrite(buf_text(&objects_list), 1, objects_list.n, f) == objects_list.n;
        if (fclose(f) == 0 && ok)
          rename(tmp.p, list.p);
        else
          unlink(tmp.p);
      }
      free(lists.p);
      free(tmp.p);
    }
  }
  for (size_t k = 0; k < n_parts; k++) {
    free(parts[k].p);
    free(outs[k]);
    free(errs[k]);
  }
  free(parts);
  free(outs);
  free(errs);
  free(work);
  free(list.p);
  return r;
}

// Link with lld's Mach-O linker (ld64.lld), given its arguments, each
// after a newline (the first is the linker's name). 0 when it linked, else
// 1 (lld has written why on standard error).
int32_t toolchain_link(const char *args) {
  buf_clear(&last_error);
  // the arguments cut at each newline, in a copy
  size_t n = strlen(args);
  char *copy = malloc(n + 1);
  memcpy(copy, args, n + 1);
  int argc = 1;
  for (size_t k = 0; k < n; k++)
    if (copy[k] == '\n') argc++;
  const char **argv = malloc((size_t)(argc + 1) * sizeof(char *));
  int a = 0;
  argv[a++] = copy;
  for (size_t k = 0; k < n; k++)
    if (copy[k] == '\n') {
      copy[k] = 0;
      argv[a++] = copy + k + 1;
    }
  argv[a] = NULL;
  timings_begin("link (lld)");
  int ok = toolchain_lld(argc, argv);
  timings_end("link (lld)");
  free(argv);
  free(copy);
  if (!ok) return fail("the linker failed");
  return 0;
}

// The folder of the running executable, its links followed ("" when it
// can't be found): a release finds what it ships relative to it
// (`bin/strata` → `lib/strata/…`).
const char *toolchain_exe_dir(void) {
  static char dir[PATH_MAX];
  char path[PATH_MAX];
  uint32_t size = sizeof path;
  if (_NSGetExecutablePath(path, &size) != 0 || !realpath(path, dir)) return "";
  char *slash = strrchr(dir, '/');
  if (slash) *slash = 0;
  return dir;
}
