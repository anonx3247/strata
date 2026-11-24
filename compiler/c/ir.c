// compiler/codegen's IR into LLVM through its C API (LLVMAddFunction,
// LLVMBuild*...), without LLVM's own reader of IR text (LLParser): codegen
// writes a small, regular part of the language, one instruction a line,
// which this reads in one pass over each function and builds directly, the
// values left unnamed (the context discards names). Several times faster
// than LLParser on compiler/'s 58 MB (it lexes and checks the whole
// language, names every value and keeps tables of them).
//
// What this doesn't know (a construct codegen has never written) makes it
// give up, and the caller (`toolchain_module`) reads the text with LLParser
// instead, as before: the result is the same module either way.
// STRATA_IR_CHECK=1 builds both and compares them printed (with the values
// named, as LLParser names them), to test this reader.

#include <llvm-c/Core.h>
#include <llvm-c/IRReader.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int32_t timings_begin(const char *name);
int32_t timings_end(const char *name);

// ---- names to values: open addressing over (bytes, length) keys ----

typedef struct {
  const char *key;
  uint32_t len;
  uint32_t stamp;
  void *val;
} Slot;

typedef struct {
  Slot *slots;
  uint32_t cap, count, stamp;
} Table;

static uint64_t hash_bytes(const char *s, uint32_t n) {
  uint64_t h = 1469598103934665603ull;
  for (uint32_t i = 0; i < n; i++) h = (h ^ (unsigned char)s[i]) * 1099511628211ull;
  return h ^ (h >> 29);
}

static void table_init(Table *t, uint32_t cap) {
  t->cap = cap;
  t->count = 0;
  t->stamp = 1;
  t->slots = calloc(cap, sizeof(Slot));
}

// Empty the table (in O(1): slots of an older stamp are empty).
static void table_clear(Table *t) {
  t->stamp++;
  t->count = 0;
}

static Slot *table_find(Table *t, const char *k, uint32_t n) {
  uint32_t mask = t->cap - 1;
  for (uint32_t i = (uint32_t)hash_bytes(k, n) & mask;; i = (i + 1) & mask) {
    Slot *s = &t->slots[i];
    if (s->stamp != t->stamp) return s;
    if (s->len == n && memcmp(s->key, k, n) == 0) return s;
  }
}

static void *table_get(Table *t, const char *k, uint32_t n) {
  Slot *s = table_find(t, k, n);
  return s->stamp == t->stamp ? s->val : NULL;
}

static void table_put(Table *t, const char *k, uint32_t n, void *v) {
  if ((t->count + 1) * 2 > t->cap) {
    Slot *old = t->slots;
    uint32_t oldcap = t->cap, stamp = t->stamp;
    t->cap *= 2;
    t->slots = calloc(t->cap, sizeof(Slot));
    t->count = 0;
    for (uint32_t i = 0; i < oldcap; i++)
      if (old[i].stamp == stamp) {
        Slot *s = table_find(t, old[i].key, old[i].len);
        *s = old[i];
        t->count++;
      }
    free(old);
  }
  Slot *s = table_find(t, k, n);
  if (s->stamp != t->stamp) {
    s->key = k;
    s->len = n;
    s->stamp = t->stamp;
    t->count++;
  }
  s->val = v;
}

// ---- the reader ----

// A phi's incoming value named before it is defined (a loop's back edge):
// added when the function ends.
typedef struct {
  LLVMValueRef phi, x;  // (x NULL: `name`, found at the end)
  const char *name;
  uint32_t len;
  LLVMBasicBlockRef from;
} Pending;

typedef struct {
  const char *p, *end, *line;
  int lineno;
  LLVMContextRef ctx;
  LLVMModuleRef mod;
  LLVMBuilderRef b;
  int names;  // name values as the text does (STRATA_IR_CHECK)
  Table globals, locals, blocks;
  Pending *pending;
  int npending, cappending;
  int made, placed;  // blocks named, and those placed in the function
  // a stand-in value of each type (`hole`), in a module of their own
  LLVMModuleRef holes;
  LLVMTypeRef hole_types[16];
  LLVMValueRef hole_values[16];
  int nholes;
  LLVMTypeRef t_void, t_i1, t_i8, t_i16, t_i32, t_i64, t_ptr, t_double, t_float;
  LLVMValueRef fn;
  // scratch arrays (call arguments, gep indices, aggregate elements)
  char err[512];
  unsigned k_zeroext, k_signext, k_noreturn, k_noinline, k_optnone, k_alwaysinline, k_nounwind;
} R;

#define FAIL(r, ...)                                                   \
  do {                                                                 \
    if (!(r)->err[0]) snprintf((r)->err, sizeof (r)->err, __VA_ARGS__); \
    return 0;                                                          \
  } while (0)

static void spaces(R *r) {
  while (r->p < r->end && (*r->p == ' ' || *r->p == '\t')) r->p++;
}

// The next word, if it is `w` (then skipped).
static int word(R *r, const char *w) {
  spaces(r);
  size_t n = strlen(w);
  if ((size_t)(r->end - r->p) < n || memcmp(r->p, w, n) != 0) return 0;
  char c = r->p + n < r->end ? r->p[n] : 0;
  if ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_' || c == '.') return 0;
  r->p += n;
  return 1;
}

static int punct(R *r, char c) {
  spaces(r);
  if (r->p < r->end && *r->p == c) {
    r->p++;
    return 1;
  }
  return 0;
}

static int is_name_char(char c) {
  return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_' || c == '.' ||
         c == '$' || c == '-';
}

// A name after its sigil (`%x.3`, `@f`, `@"a b"`): its bytes, in the text
// (a quoted name may not hold escapes).
static int name(R *r, const char **s, uint32_t *n) {
  if (r->p < r->end && *r->p == '"') {
    const char *q = ++r->p;
    while (r->p < r->end && *r->p != '"') {
      if (*r->p == '\\' || *r->p == '\n') return 0;
      r->p++;
    }
    if (r->p >= r->end) return 0;
    *s = q;
    *n = (uint32_t)(r->p - q);
    r->p++;
    return 1;
  }
  const char *q = r->p;
  while (r->p < r->end && is_name_char(*r->p)) r->p++;
  *s = q;
  *n = (uint32_t)(r->p - q);
  return *n > 0;
}

static int number(R *r, long long *v) {
  spaces(r);
  const char *q = r->p;
  int neg = 0;
  if (q < r->end && *q == '-') neg = 1, q++;
  if (q >= r->end || *q < '0' || *q > '9') return 0;
  unsigned long long x = 0;
  while (q < r->end && *q >= '0' && *q <= '9') x = x * 10 + (unsigned long long)(*q++ - '0');
  r->p = q;
  *v = (long long)(neg ? 0ull - x : x);
  return 1;
}

static LLVMTypeRef type(R *r);

static LLVMTypeRef type(R *r) {
  spaces(r);
  if (r->p >= r->end) return NULL;
  char c = *r->p;
  if (c == 'p' && word(r, "ptr")) return r->t_ptr;
  if (c == 'i') {
    const char *q = r->p + 1;
    int bits = 0;
    while (q < r->end && *q >= '0' && *q <= '9') bits = bits * 10 + (*q++ - '0');
    if (q == r->p + 1 || (q < r->end && is_name_char(*q))) return NULL;
    r->p = q;
    switch (bits) {
      case 1: return r->t_i1;
      case 8: return r->t_i8;
      case 16: return r->t_i16;
      case 32: return r->t_i32;
      case 64: return r->t_i64;
      default: return LLVMIntTypeInContext(r->ctx, (unsigned)bits);
    }
  }
  if (c == 'd' && word(r, "double")) return r->t_double;
  if (c == 'v' && word(r, "void")) return r->t_void;
  if (c == 'f' && word(r, "float")) return r->t_float;
  if (c == '{') {
    r->p++;
    LLVMTypeRef el[64];
    unsigned n = 0;
    if (!punct(r, '}')) {
      do {
        if (n == 64) return NULL;
        if (!(el[n++] = type(r))) return NULL;
      } while (punct(r, ','));
      if (!punct(r, '}')) return NULL;
    }
    return LLVMStructTypeInContext(r->ctx, el, n, 0);
  }
  if (c == '[') {
    r->p++;
    long long n;
    if (!number(r, &n) || !word(r, "x")) return NULL;
    LLVMTypeRef el = type(r);
    if (!el || !punct(r, ']')) return NULL;
    return LLVMArrayType2(el, (uint64_t)n);
  }
  return NULL;
}

static LLVMValueRef value(R *r, LLVMTypeRef t);

// A value of type `t` that is no constant (a function's parameter, in a
// module of its own): what an instruction is built with in place of a
// constant operand, then given the constant, so that LLVM's builder, which
// folds an instruction of constants into a constant, builds it as written,
// as LLParser does (`shl i64 0, 1` stays an instruction): the module is the
// same whichever reads the text.
static LLVMValueRef hole(R *r, LLVMTypeRef t) {
  for (int i = 0; i < r->nholes; i++)
    if (r->hole_types[i] == t) return r->hole_values[i];
  if (!r->holes) r->holes = LLVMModuleCreateWithNameInContext("strata.holes", r->ctx);
  LLVMValueRef f = LLVMAddFunction(r->holes, "", LLVMFunctionType(r->t_void, &t, 1, 0));
  LLVMValueRef v = LLVMGetParam(f, 0);
  if (r->nholes < 16) {
    r->hole_types[r->nholes] = t;
    r->hole_values[r->nholes++] = v;
  }
  return v;
}

// A constant of type `t` (no `%name`).
static LLVMValueRef constant(R *r, LLVMTypeRef t) {
  spaces(r);
  if (r->p >= r->end) return NULL;
  char c = *r->p;
  LLVMTypeKind k = LLVMGetTypeKind(t);
  if (c == '@') {
    r->p++;
    const char *s;
    uint32_t n;
    if (!name(r, &s, &n)) return NULL;
    return table_get(&r->globals, s, n);
  }
  if ((c >= '0' && c <= '9') || c == '-') {
    if (k == LLVMIntegerTypeKind) {
      long long v;
      if (!number(r, &v)) return NULL;
      if (r->p < r->end && (*r->p == '.' || *r->p == 'e')) return NULL;
      return LLVMConstInt(t, (unsigned long long)v, 1);
    }
    if (k == LLVMDoubleTypeKind) {
      if (c == '0' && r->p + 1 < r->end && r->p[1] == 'x') {
        // its 64 bits in hexadecimal
        const char *q = r->p + 2;
        unsigned long long bits = 0;
        int digits = 0;
        for (; q < r->end; q++, digits++) {
          char d = *q;
          int v = d >= '0' && d <= '9' ? d - '0' : d >= 'A' && d <= 'F' ? d - 'A' + 10 : d >= 'a' && d <= 'f' ? d - 'a' + 10 : -1;
          if (v < 0) break;
          bits = bits << 4 | (unsigned)v;
        }
        if (digits != 16) return NULL;
        r->p = q;
        return LLVMConstBitCast(LLVMConstInt(r->t_i64, bits, 0), r->t_double);
      }
      char buf[64];
      const char *q = r->p;
      size_t n = 0;
      while (q < r->end && n < 63 && (is_name_char(*q) || *q == '+')) buf[n++] = *q++;
      buf[n] = 0;
      char *stop;
      double d = strtod(buf, &stop);
      if (stop != buf + n) return NULL;
      r->p = q;
      return LLVMConstReal(t, d);
    }
    return NULL;
  }
  switch (c) {
    case 't':
      if (word(r, "true")) return k == LLVMIntegerTypeKind ? LLVMConstInt(t, 1, 0) : NULL;
      break;
    case 'f':
      if (word(r, "false")) return k == LLVMIntegerTypeKind ? LLVMConstInt(t, 0, 0) : NULL;
      break;
    case 'n':
      if (word(r, "null")) return k == LLVMPointerTypeKind ? LLVMConstPointerNull(t) : NULL;
      break;
    case 'p':
      if (word(r, "poison")) return LLVMGetPoison(t);
      break;
    case 'u':
      if (word(r, "undef")) return LLVMGetUndef(t);
      break;
    case 'z':
      if (word(r, "zeroinitializer")) return LLVMConstNull(t);
      break;
    case 'g':
      // getelementptr [inbounds] (T, ptr @g, i64 N...)
      if (word(r, "getelementptr")) {
        int inb = word(r, "inbounds");
        if (!punct(r, '(')) return NULL;
        LLVMTypeRef et = type(r);
        if (!et || !punct(r, ',')) return NULL;
        LLVMTypeRef bt = type(r);
        if (!bt) return NULL;
        LLVMValueRef base = constant(r, bt);
        if (!base) return NULL;
        LLVMValueRef idx[8];
        unsigned n = 0;
        while (punct(r, ',')) {
          LLVMTypeRef it = type(r);
          if (!it || n == 8 || !(idx[n++] = constant(r, it))) return NULL;
        }
        if (!punct(r, ')')) return NULL;
        return inb ? LLVMConstInBoundsGEP2(et, base, idx, n) : LLVMConstGEP2(et, base, idx, n);
      }
      break;
    case 'c':
      // c"bytes", \XX escaped
      if (r->p + 1 < r->end && r->p[1] == '"' && k == LLVMArrayTypeKind) {
        r->p += 2;
        uint64_t len = LLVMGetArrayLength2(t);
        char small[256];
        char *buf = len <= sizeof small ? small : malloc(len);
        uint64_t n = 0;
        while (r->p < r->end && *r->p != '"') {
          char ch = *r->p++;
          if (ch == '\\') {
            if (r->end - r->p < 2) break;
            int v = 0;
            for (int i = 0; i < 2; i++) {
              char d = *r->p++;
              v = v * 16 + (d >= '0' && d <= '9' ? d - '0' : d >= 'A' && d <= 'F' ? d - 'A' + 10 : d >= 'a' && d <= 'f' ? d - 'a' + 10 : 0);
            }
            ch = (char)v;
          }
          if (n < len) buf[n] = ch;
          n++;
        }
        LLVMValueRef v = NULL;
        if (r->p < r->end && n == len) {
          r->p++;
          v = LLVMConstStringInContext2(r->ctx, buf, len, 1);
        }
        if (buf != small) free(buf);
        return v;
      }
      break;
    case '{':
      if (k == LLVMStructTypeKind) {
        r->p++;
        unsigned n = LLVMCountStructElementTypes(t);
        LLVMValueRef small[16];
        LLVMValueRef *el = n <= 16 ? small : malloc(n * sizeof *el);
        LLVMValueRef v = NULL;
        unsigned i = 0;
        for (; i < n; i++) {
          if (i && !punct(r, ',')) break;
          LLVMTypeRef et = type(r);
          if (!et || et != LLVMStructGetTypeAtIndex(t, i) || !(el[i] = constant(r, et))) break;
        }
        if (i == n && punct(r, '}')) v = LLVMConstStructInContext(r->ctx, el, n, 0);
        if (el != small) free(el);
        return v;
      }
      break;
    case '[':
      if (k == LLVMArrayTypeKind) {
        r->p++;
        uint64_t n = LLVMGetArrayLength2(t);
        LLVMTypeRef et = LLVMGetElementType(t);
        LLVMValueRef *el = malloc((n ? n : 1) * sizeof *el);
        LLVMValueRef v = NULL;
        uint64_t i = 0;
        for (; i < n; i++) {
          if (i && !punct(r, ',')) break;
          if (type(r) != et || !(el[i] = constant(r, et))) break;
        }
        if (i == n && punct(r, ']')) v = LLVMConstArray2(et, el, n);
        free(el);
        return v;
      }
      break;
  }
  return NULL;
}

// An operand of type `t`: a local (`%x`), or a constant.
static LLVMValueRef value(R *r, LLVMTypeRef t) {
  spaces(r);
  if (r->p < r->end && *r->p == '%') {
    r->p++;
    const char *s;
    uint32_t n;
    if (!name(r, &s, &n)) return NULL;
    LLVMValueRef v = table_get(&r->locals, s, n);
    if (!v) snprintf(r->err, sizeof r->err, "%%%.*s used before it is defined", (int)n, s);
    return v;
  }
  return constant(r, t);
}

static LLVMBasicBlockRef block_named(R *r, const char *s, uint32_t n) {
  LLVMBasicBlockRef bb = table_get(&r->blocks, s, n);
  if (!bb) {
    bb = LLVMCreateBasicBlockInContext(r->ctx, "");
    table_put(&r->blocks, s, n, bb);
    r->made++;
  }
  return bb;
}

// `label %l`
static LLVMBasicBlockRef label(R *r) {
  if (!word(r, "label")) return NULL;
  spaces(r);
  if (r->p >= r->end || *r->p != '%') return NULL;
  r->p++;
  const char *s;
  uint32_t n;
  if (!name(r, &s, &n)) return NULL;
  return block_named(r, s, n);
}

static void set_name(R *r, LLVMValueRef v, const char *s, uint32_t n) {
  if (r->names) LLVMSetValueName2(v, s, n);
}

// `, align N` after an instruction (when written).
static int align_of(R *r, LLVMValueRef v) {
  const char *save = r->p;
  if (punct(r, ',')) {
    if (!word(r, "align")) {
      r->p = save;
      return 1;
    }
    long long a;
    if (!number(r, &a)) return 0;
    LLVMSetAlignment(v, (unsigned)a);
  }
  return 1;
}

static int int_pred(R *r, LLVMIntPredicate *p) {
  static const struct { const char *w; LLVMIntPredicate p; } ps[] = {
      {"eq", LLVMIntEQ},   {"ne", LLVMIntNE},   {"slt", LLVMIntSLT}, {"sle", LLVMIntSLE}, {"sgt", LLVMIntSGT},
      {"sge", LLVMIntSGE}, {"ult", LLVMIntULT}, {"ule", LLVMIntULE}, {"ugt", LLVMIntUGT}, {"uge", LLVMIntUGE}};
  for (size_t i = 0; i < sizeof ps / sizeof *ps; i++)
    if (word(r, ps[i].w)) return *p = ps[i].p, 1;
  return 0;
}

static int real_pred(R *r, LLVMRealPredicate *p) {
  static const struct { const char *w; LLVMRealPredicate p; } ps[] = {
      {"oeq", LLVMRealOEQ}, {"ogt", LLVMRealOGT}, {"oge", LLVMRealOGE}, {"olt", LLVMRealOLT},
      {"ole", LLVMRealOLE}, {"one", LLVMRealONE}, {"ord", LLVMRealORD}, {"uno", LLVMRealUNO},
      {"ueq", LLVMRealUEQ}, {"ugt", LLVMRealUGT}, {"uge", LLVMRealUGE}, {"ult", LLVMRealULT},
      {"ule", LLVMRealULE}, {"une", LLVMRealUNE}, {"true", LLVMRealPredicateTrue}, {"false", LLVMRealPredicateFalse}};
  for (size_t i = 0; i < sizeof ps / sizeof *ps; i++)
    if (word(r, ps[i].w)) return *p = ps[i].p, 1;
  return 0;
}

static int ordering(R *r, LLVMAtomicOrdering *o) {
  if (word(r, "acq_rel")) *o = LLVMAtomicOrderingAcquireRelease;
  else if (word(r, "seq_cst")) *o = LLVMAtomicOrderingSequentiallyConsistent;
  else if (word(r, "monotonic")) *o = LLVMAtomicOrderingMonotonic;
  else if (word(r, "unordered")) *o = LLVMAtomicOrderingUnordered;
  else if (word(r, "acquire")) *o = LLVMAtomicOrderingAcquire;
  else if (word(r, "release")) *o = LLVMAtomicOrderingRelease;
  else return 0;
  return 1;
}

static const struct {
  const char *w;
  LLVMOpcode op;
} binops[] = {{"add", LLVMAdd},   {"sub", LLVMSub},   {"mul", LLVMMul},   {"sdiv", LLVMSDiv}, {"udiv", LLVMUDiv},
              {"srem", LLVMSRem}, {"urem", LLVMURem}, {"shl", LLVMShl},   {"lshr", LLVMLShr}, {"ashr", LLVMAShr},
              {"and", LLVMAnd},   {"or", LLVMOr},     {"xor", LLVMXor},   {"fadd", LLVMFAdd}, {"fsub", LLVMFSub},
              {"fmul", LLVMFMul}, {"fdiv", LLVMFDiv}, {"frem", LLVMFRem}};

static const struct {
  const char *w;
  LLVMOpcode op;
} casts[] = {{"trunc", LLVMTrunc},       {"zext", LLVMZExt},         {"sext", LLVMSExt},
             {"inttoptr", LLVMIntToPtr}, {"ptrtoint", LLVMPtrToInt}, {"bitcast", LLVMBitCast},
             {"sitofp", LLVMSIToFP},     {"fptosi", LLVMFPToSI},     {"uitofp", LLVMUIToFP},
             {"fptoui", LLVMFPToUI},     {"fpext", LLVMFPExt},       {"fptrunc", LLVMFPTrunc}};

// The call after `call` (`[zeroext] RET CALLEE(T a, ...)`): the instruction.
static LLVMValueRef call(R *r, LLVMTailCallKind tail) {
  int zext = word(r, "zeroext");
  LLVMTypeRef ret = type(r);
  if (!ret) FAIL(r, "a call's type");
  spaces(r);
  if (r->p < r->end && *r->p == '(') FAIL(r, "a call with its function's type written");
  LLVMValueRef callee = value(r, r->t_ptr);
  if (!callee || !punct(r, '(')) FAIL(r, "a call's callee");
  LLVMValueRef args[64];
  LLVMTypeRef params[64];
  unsigned n = 0;
  if (!punct(r, ')')) {
    do {
      if (n == 64) FAIL(r, "a call of more than 64 arguments");
      if (!(params[n] = type(r))) FAIL(r, "an argument's type");
      if (!(args[n] = value(r, params[n]))) FAIL(r, "an argument");
      n++;
    } while (punct(r, ','));
    if (!punct(r, ')')) FAIL(r, "a call's `)`");
  }
  LLVMTypeRef fty = LLVMFunctionType(ret, params, n, 0);
  LLVMValueRef c = LLVMBuildCall2(r->b, fty, callee, args, n, "");
  if (zext) LLVMAddCallSiteAttribute(c, LLVMAttributeReturnIndex, LLVMCreateEnumAttribute(r->ctx, r->k_zeroext, 0));
  if (tail != LLVMTailCallKindNone) LLVMSetTailCallKind(c, tail);
  return c;
}

// One instruction (the line's text after its indentation): 1 when built.
static int instruction(R *r) {
  // `%x = ...`: the name it defines
  const char *def = NULL;
  uint32_t deflen = 0;
  if (*r->p == '%') {
    r->p++;
    if (!name(r, &def, &deflen) || !punct(r, '=')) FAIL(r, "an instruction's name");
    spaces(r);
  }
  LLVMValueRef v = NULL;
  const char *w = r->p;
  switch (*w) {
    case 'l':
      if (word(r, "load")) {
        int atomic = word(r, "atomic");
        LLVMTypeRef t = type(r);
        if (!t || !punct(r, ',') || type(r) != r->t_ptr) FAIL(r, "a load");
        LLVMValueRef p = value(r, r->t_ptr);
        if (!p) FAIL(r, "a load's pointer");
        v = LLVMBuildLoad2(r->b, t, p, "");
        if (atomic) {
          LLVMAtomicOrdering o;
          if (!ordering(r, &o)) FAIL(r, "a load's ordering");
          LLVMSetOrdering(v, o);
        }
        if (!align_of(r, v)) FAIL(r, "a load's alignment");
      }
      break;
    case 's':
      if (word(r, "store")) {
        int atomic = word(r, "atomic");
        LLVMTypeRef t = type(r);
        if (!t) FAIL(r, "a store's type");
        LLVMValueRef x = value(r, t);
        if (!x || !punct(r, ',') || type(r) != r->t_ptr) FAIL(r, "a store's value");
        LLVMValueRef p = value(r, r->t_ptr);
        if (!p) FAIL(r, "a store's pointer");
        LLVMValueRef s = LLVMBuildStore(r->b, x, p);
        if (atomic) {
          LLVMAtomicOrdering o;
          if (!ordering(r, &o)) FAIL(r, "a store's ordering");
          LLVMSetOrdering(s, o);
        }
        if (!align_of(r, s)) FAIL(r, "a store's alignment");
        return 1;
      }
      if (word(r, "switch")) {
        LLVMTypeRef t = type(r);
        LLVMValueRef x = t ? value(r, t) : NULL;
        if (!x || !punct(r, ',')) FAIL(r, "a switch's value");
        LLVMBasicBlockRef d = label(r);
        if (!d || !punct(r, '[')) FAIL(r, "a switch's default");
        LLVMValueRef sw = LLVMBuildSwitch(r->b, x, d, 4);
        while (!punct(r, ']')) {
          LLVMTypeRef ct = type(r);
          LLVMValueRef c = ct ? constant(r, ct) : NULL;
          if (!c || !punct(r, ',')) FAIL(r, "a switch's case");
          LLVMBasicBlockRef to = label(r);
          if (!to) FAIL(r, "a switch's case label");
          LLVMAddCase(sw, c, to);
        }
        return 1;
      }
      if (word(r, "select")) {
        LLVMTypeRef ct = type(r);
        LLVMValueRef c = ct ? value(r, ct) : NULL;
        if (!c || !punct(r, ',')) FAIL(r, "a select's condition");
        LLVMTypeRef t = type(r);
        LLVMValueRef a = t ? value(r, t) : NULL;
        if (!a || !punct(r, ',') || type(r) != t) FAIL(r, "a select's value");
        LLVMValueRef b = value(r, t);
        if (!b) FAIL(r, "a select's value");
        int fold = LLVMIsConstant(c) && LLVMIsConstant(a) && LLVMIsConstant(b);
        v = LLVMBuildSelect(r->b, fold ? hole(r, ct) : c, a, b, "");
        if (fold) LLVMSetOperand(v, 0, c);
      }
      break;
    case 'g':
      if (word(r, "getelementptr")) {
        int inb = word(r, "inbounds");
        LLVMTypeRef et = type(r);
        if (!et || !punct(r, ',') || type(r) != r->t_ptr) FAIL(r, "a getelementptr");
        LLVMValueRef p = value(r, r->t_ptr);
        if (!p) FAIL(r, "a getelementptr's pointer");
        LLVMValueRef idx[16];
        unsigned n = 0;
        while (punct(r, ',')) {
          LLVMTypeRef it = type(r);
          if (!it || n == 16 || !(idx[n++] = value(r, it))) FAIL(r, "a getelementptr's index");
        }
        int fold = LLVMIsConstant(p);
        for (unsigned i = 0; i < n; i++) fold = fold && LLVMIsConstant(idx[i]);
        LLVMValueRef p0 = fold ? hole(r, r->t_ptr) : p;
        v = inb ? LLVMBuildInBoundsGEP2(r->b, et, p0, idx, n, "") : LLVMBuildGEP2(r->b, et, p0, idx, n, "");
        if (fold) LLVMSetOperand(v, 0, p);
      }
      break;
    case 'c':
      if (word(r, "call")) {
        v = call(r, LLVMTailCallKindNone);
        if (!v) return 0;
        if (!def) return 1;
      }
      break;
    case 'm':
      if (word(r, "musttail")) {
        if (!word(r, "call")) FAIL(r, "musttail without a call");
        v = call(r, LLVMTailCallKindMustTail);
        if (!v) return 0;
        if (!def) return 1;
      }
      break;
    case 't':
      if (word(r, "tail")) {
        if (!word(r, "call")) FAIL(r, "tail without a call");
        v = call(r, LLVMTailCallKindTail);
        if (!v) return 0;
        if (!def) return 1;
      }
      break;
    case 'b':
      if (word(r, "br")) {
        spaces(r);
        if (r->p < r->end && *r->p == 'l') {
          LLVMBasicBlockRef to = label(r);
          if (!to) FAIL(r, "a branch's label");
          LLVMBuildBr(r->b, to);
          return 1;
        }
        LLVMTypeRef t = type(r);
        LLVMValueRef c = t ? value(r, t) : NULL;
        if (!c || !punct(r, ',')) FAIL(r, "a branch's condition");
        LLVMBasicBlockRef a = label(r);
        if (!a || !punct(r, ',')) FAIL(r, "a branch's label");
        LLVMBasicBlockRef b = label(r);
        if (!b) FAIL(r, "a branch's label");
        LLVMBuildCondBr(r->b, c, a, b);
        return 1;
      }
      break;
    case 'r':
      if (word(r, "ret")) {
        LLVMTypeRef t = type(r);
        if (!t) FAIL(r, "a return's type");
        if (t == r->t_void) {
          LLVMBuildRetVoid(r->b);
          return 1;
        }
        LLVMValueRef x = value(r, t);
        if (!x) FAIL(r, "a return's value");
        LLVMBuildRet(r->b, x);
        return 1;
      }
      break;
    case 'u':
      if (word(r, "unreachable")) {
        LLVMBuildUnreachable(r->b);
        return 1;
      }
      break;
    case 'a':
      if (word(r, "alloca")) {
        LLVMTypeRef t = type(r);
        if (!t) FAIL(r, "an alloca's type");
        v = LLVMBuildAlloca(r->b, t, "");
        if (!align_of(r, v)) FAIL(r, "an alloca's alignment");
      } else if (word(r, "atomicrmw")) {
        static const struct { const char *w; LLVMAtomicRMWBinOp op; } ops[] = {
            {"xchg", LLVMAtomicRMWBinOpXchg}, {"add", LLVMAtomicRMWBinOpAdd}, {"sub", LLVMAtomicRMWBinOpSub},
            {"and", LLVMAtomicRMWBinOpAnd},   {"or", LLVMAtomicRMWBinOpOr},   {"xor", LLVMAtomicRMWBinOpXor}};
        size_t i = 0;
        for (; i < sizeof ops / sizeof *ops; i++)
          if (word(r, ops[i].w)) break;
        if (i == sizeof ops / sizeof *ops || type(r) != r->t_ptr) FAIL(r, "an atomicrmw");
        LLVMValueRef p = value(r, r->t_ptr);
        if (!p || !punct(r, ',')) FAIL(r, "an atomicrmw's pointer");
        LLVMTypeRef t = type(r);
        LLVMValueRef x = t ? value(r, t) : NULL;
        if (!x) FAIL(r, "an atomicrmw's value");
        LLVMAtomicOrdering o;
        if (!ordering(r, &o)) FAIL(r, "an atomicrmw's ordering");
        v = LLVMBuildAtomicRMW(r->b, ops[i].op, p, x, o, 0);
        if (!align_of(r, v)) FAIL(r, "an atomicrmw's alignment");
      }
      break;
    case 'i':
      if (word(r, "icmp")) {
        LLVMIntPredicate pr;
        if (!int_pred(r, &pr)) FAIL(r, "an icmp's predicate");
        LLVMTypeRef t = type(r);
        LLVMValueRef a = t ? value(r, t) : NULL;
        if (!a || !punct(r, ',')) FAIL(r, "an icmp's operand");
        LLVMValueRef b = value(r, t);
        if (!b) FAIL(r, "an icmp's operand");
        int fold = LLVMIsConstant(a) && LLVMIsConstant(b);
        v = LLVMBuildICmp(r->b, pr, fold ? hole(r, t) : a, b, "");
        if (fold) LLVMSetOperand(v, 0, a);
      }
      break;
    case 'f':
      if (word(r, "fcmp")) {
        LLVMRealPredicate pr;
        if (!real_pred(r, &pr)) FAIL(r, "an fcmp's predicate");
        LLVMTypeRef t = type(r);
        LLVMValueRef a = t ? value(r, t) : NULL;
        if (!a || !punct(r, ',')) FAIL(r, "an fcmp's operand");
        LLVMValueRef b = value(r, t);
        if (!b) FAIL(r, "an fcmp's operand");
        int fold = LLVMIsConstant(a) && LLVMIsConstant(b);
        v = LLVMBuildFCmp(r->b, pr, fold ? hole(r, t) : a, b, "");
        if (fold) LLVMSetOperand(v, 0, a);
      } else if (word(r, "fneg")) {
        LLVMTypeRef t = type(r);
        LLVMValueRef a = t ? value(r, t) : NULL;
        if (!a) FAIL(r, "an fneg's operand");
        int fold = LLVMIsConstant(a);
        v = LLVMBuildFNeg(r->b, fold ? hole(r, t) : a, "");
        if (fold) LLVMSetOperand(v, 0, a);
      }
      break;
    case 'p':
      if (word(r, "phi")) {
        LLVMTypeRef t = type(r);
        if (!t) FAIL(r, "a phi's type");
        v = LLVMBuildPhi(r->b, t, "");
        int first = r->npending, late = 0;
        do {
          if (!punct(r, '[')) FAIL(r, "a phi's `[`");
          spaces(r);
          LLVMValueRef x = NULL;
          const char *s = NULL;
          uint32_t n = 0;
          if (r->p < r->end && *r->p == '%') {
            r->p++;
            if (!name(r, &s, &n)) FAIL(r, "a phi's value");
            x = table_get(&r->locals, s, n);
          } else if (!(x = constant(r, t))) {
            FAIL(r, "a phi's value");
          }
          if (!punct(r, ',') || !punct(r, '%')) FAIL(r, "a phi's block");
          const char *bs;
          uint32_t bn;
          if (!name(r, &bs, &bn)) FAIL(r, "a phi's block");
          LLVMBasicBlockRef from = block_named(r, bs, bn);
          if (!punct(r, ']')) FAIL(r, "a phi's `]`");
          if (!x) late = 1;
          if (r->npending == r->cappending) {
            r->cappending = r->cappending ? r->cappending * 2 : 64;
            r->pending = realloc(r->pending, r->cappending * sizeof *r->pending);
          }
          r->pending[r->npending++] = (Pending){v, x, s, n, from};
        } while (punct(r, ','));
        // (all known: added now; else at the function's end, in the order
        // written, as LLParser adds them)
        if (!late) {
          for (int i = first; i < r->npending; i++) LLVMAddIncoming(v, &r->pending[i].x, &r->pending[i].from, 1);
          r->npending = first;
        }
      }
      break;
    case 'e':
      if (word(r, "extractvalue")) {
        LLVMTypeRef t = type(r);
        LLVMValueRef a = t ? value(r, t) : NULL;
        if (!a) FAIL(r, "an extractvalue's operand");
        v = a;
        while (punct(r, ',')) {
          long long i;
          if (!number(r, &i)) FAIL(r, "an extractvalue's index");
          int fold = LLVMIsConstant(v);
          LLVMValueRef agg = v;
          v = LLVMBuildExtractValue(r->b, fold ? hole(r, LLVMTypeOf(v)) : v, (unsigned)i, "");
          if (fold) LLVMSetOperand(v, 0, agg);
        }
        if (v == a) FAIL(r, "an extractvalue without an index");
      }
      break;
  }
  if (!v) {
    r->p = w;
    for (size_t i = 0; i < sizeof binops / sizeof *binops; i++) {
      if (!word(r, binops[i].w)) continue;
      int nsw = 0, nuw = 0, exact = 0;
      for (;;) {
        if (word(r, "nsw")) nsw = 1;
        else if (word(r, "nuw")) nuw = 1;
        else if (word(r, "exact")) exact = 1;
        else break;
      }
      LLVMTypeRef t = type(r);
      LLVMValueRef a = t ? value(r, t) : NULL;
      if (!a || !punct(r, ',')) FAIL(r, "an operand of %s", binops[i].w);
      LLVMValueRef b = value(r, t);
      if (!b) FAIL(r, "an operand of %s", binops[i].w);
      int fold = LLVMIsConstant(a) && LLVMIsConstant(b);
      v = LLVMBuildBinOp(r->b, binops[i].op, fold ? hole(r, t) : a, b, "");
      if (fold) LLVMSetOperand(v, 0, a);
      if (nsw) LLVMSetNSW(v, 1);
      if (nuw) LLVMSetNUW(v, 1);
      if (exact) LLVMSetExact(v, 1);
      break;
    }
  }
  if (!v) {
    r->p = w;
    for (size_t i = 0; i < sizeof casts / sizeof *casts; i++) {
      if (!word(r, casts[i].w)) continue;
      LLVMTypeRef t = type(r);
      LLVMValueRef a = t ? value(r, t) : NULL;
      if (!a || !word(r, "to")) FAIL(r, "a %s", casts[i].w);
      LLVMTypeRef to = type(r);
      if (!to) FAIL(r, "a %s's type", casts[i].w);
      int fold = LLVMIsConstant(a);
      v = LLVMBuildCast(r->b, casts[i].op, fold ? hole(r, t) : a, to, "");
      if (fold) LLVMSetOperand(v, 0, a);
      break;
    }
  }
  if (!v) FAIL(r, "an instruction this reader doesn't know");
  if (def) {
    if (LLVMTypeOf(v) == r->t_void) FAIL(r, "a name for nothing");
    table_put(&r->locals, def, deflen, v);
    if (LLVMIsAInstruction(v)) set_name(r, v, def, deflen);
  }
  return 1;
}

// The rest of the line must be empty (or a comment).
static int line_end(R *r) {
  spaces(r);
  if (r->p < r->end && *r->p == ';') {
    while (r->p < r->end && *r->p != '\n') r->p++;
  }
  if (r->p < r->end && *r->p != '\n') FAIL(r, "more on the line than this reader knows");
  if (r->p < r->end) r->p++;
  r->lineno++;
  return 1;
}

static void skip_line(R *r) {
  while (r->p < r->end && *r->p != '\n') r->p++;
  if (r->p < r->end) r->p++;
  r->lineno++;
}

// A function's header after `define`/`declare`: linkage, return, name,
// parameters (named when `define`), attributes. The function (made the
// first time: the module's first pass), with `fn_out`.
static int header(R *r, int define, LLVMValueRef *fn_out, int make) {
  LLVMLinkage linkage = LLVMExternalLinkage;
  if (word(r, "internal")) linkage = LLVMInternalLinkage;
  else if (word(r, "private")) linkage = LLVMPrivateLinkage;
  int zext = word(r, "zeroext");
  LLVMTypeRef ret = type(r);
  if (!ret || !punct(r, '@')) FAIL(r, "a function's header");
  const char *s;
  uint32_t n;
  if (!name(r, &s, &n) || !punct(r, '(')) FAIL(r, "a function's name");
  LLVMTypeRef params[64];
  const char *pnames[64];
  uint32_t plens[64];
  unsigned np = 0;
  if (!punct(r, ')')) {
    do {
      if (np == 64) FAIL(r, "a function of more than 64 parameters");
      if (!(params[np] = type(r))) FAIL(r, "a parameter's type");
      pnames[np] = NULL;
      plens[np] = 0;
      spaces(r);
      if (r->p < r->end && *r->p == '%') {
        r->p++;
        if (!name(r, &pnames[np], &plens[np])) FAIL(r, "a parameter's name");
      }
      np++;
    } while (punct(r, ','));
    if (!punct(r, ')')) FAIL(r, "a function's `)`");
  }
  LLVMValueRef fn;
  if (make) {
    if (table_get(&r->globals, s, n)) FAIL(r, "@%.*s defined twice", (int)n, s);
    char small[256];
    char *z = n < sizeof small ? small : malloc(n + 1);
    memcpy(z, s, n);
    z[n] = 0;
    fn = LLVMAddFunction(r->mod, z, LLVMFunctionType(ret, params, np, 0));
    if (z != small) free(z);
    // (a name taken already: LLVM renamed it)
    size_t got;
    const char *nm = LLVMGetValueName2(fn, &got);
    if (got != n || memcmp(nm, s, n) != 0) FAIL(r, "@%.*s's name taken", (int)n, s);
    LLVMSetLinkage(fn, linkage);
    table_put(&r->globals, s, n, fn);
    if (zext) LLVMAddAttributeAtIndex(fn, LLVMAttributeReturnIndex, LLVMCreateEnumAttribute(r->ctx, r->k_zeroext, 0));
    // function attributes
    for (;;) {
      unsigned k = 0;
      if (word(r, "noinline")) k = r->k_noinline;
      else if (word(r, "optnone")) k = r->k_optnone;
      else if (word(r, "alwaysinline")) k = r->k_alwaysinline;
      else if (word(r, "noreturn")) k = r->k_noreturn;
      else if (word(r, "nounwind")) k = r->k_nounwind;
      else break;
      LLVMAddAttributeAtIndex(fn, LLVMAttributeFunctionIndex, LLVMCreateEnumAttribute(r->ctx, k, 0));
    }
  } else {
    fn = table_get(&r->globals, s, n);
    while (word(r, "noinline") || word(r, "optnone") || word(r, "alwaysinline") || word(r, "noreturn") ||
           word(r, "nounwind"))
      ;
    if (define) {
      table_clear(&r->locals);
      for (unsigned i = 0; i < np; i++) {
        if (!pnames[i]) FAIL(r, "a parameter without a name");
        LLVMValueRef p = LLVMGetParam(fn, i);
        table_put(&r->locals, pnames[i], plens[i], p);
        set_name(r, p, pnames[i], plens[i]);
      }
    }
  }
  if (define && !punct(r, '{')) FAIL(r, "a definition's `{`");
  *fn_out = fn;
  return 1;
}

// A function's body, after its `define ... {` line, to its `}`.
static int body(R *r, LLVMValueRef fn) {
  table_clear(&r->blocks);
  r->npending = 0;
  r->made = r->placed = 0;
  r->fn = fn;
  int open = 0;  // a block is being filled
  for (;;) {
    if (r->p >= r->end) FAIL(r, "a function without its `}`");
    const char *ls = r->p;
    r->line = ls;
    if (*ls == '}') {
      r->p++;
      if (!line_end(r)) return 0;
      break;
    }
    if (*ls == '\n' || *ls == ';') {
      skip_line(r);
      continue;
    }
    if (*ls == ' ') {
      spaces(r);
      if (r->p >= r->end || *r->p == ';' || *r->p == '\n') {
        skip_line(r);
        continue;
      }
      if (!open) {
        // (the entry block, unnamed)
        LLVMBasicBlockRef bb = LLVMAppendBasicBlockInContext(r->ctx, fn, "");
        LLVMPositionBuilderAtEnd(r->b, bb);
        open = 1;
      }
      if (!instruction(r) || !line_end(r)) return 0;
      continue;
    }
    // `label:`
    const char *s;
    uint32_t n;
    if (!name(r, &s, &n) || !punct(r, ':')) FAIL(r, "a label");
    LLVMBasicBlockRef bb = block_named(r, s, n);
    if (LLVMGetBasicBlockParent(bb)) FAIL(r, "the block %.*s twice", (int)n, s);
    LLVMAppendExistingBasicBlock(fn, bb);
    r->placed++;
    if (r->names) LLVMSetValueName2(LLVMBasicBlockAsValue(bb), s, n);
    LLVMPositionBuilderAtEnd(r->b, bb);
    open = 1;
    if (!line_end(r)) return 0;
  }
  // a block named but never written: it would be left out of the function
  if (r->made != r->placed) FAIL(r, "a label never defined");
  // the phis with values defined further on
  for (int i = 0; i < r->npending; i++) {
    Pending *pd = &r->pending[i];
    LLVMValueRef x = pd->x ? pd->x : table_get(&r->locals, pd->name, pd->len);
    if (!x) FAIL(r, "%%%.*s, in a phi, never defined", (int)pd->len, pd->name);
    LLVMAddIncoming(pd->phi, &x, &pd->from, 1);
  }
  return 1;
}

// A global's definition, after `@name = `: made in the first pass, its
// initializer set in the second.
static int global(R *r, const char *s, uint32_t n, int make) {
  LLVMLinkage linkage = LLVMExternalLinkage;
  int is_private = 0;
  if (word(r, "private")) linkage = LLVMPrivateLinkage, is_private = 1;
  else if (word(r, "internal")) linkage = LLVMInternalLinkage;
  else if (word(r, "external")) linkage = LLVMExternalLinkage;
  (void)is_private;
  int tls = word(r, "thread_local");
  int unnamed = word(r, "unnamed_addr");
  int is_const;
  if (word(r, "constant")) is_const = 1;
  else if (word(r, "global")) is_const = 0;
  else FAIL(r, "a global");
  LLVMTypeRef t = type(r);
  if (!t) FAIL(r, "a global's type");
  LLVMValueRef g;
  if (make) {
    if (table_get(&r->globals, s, n)) FAIL(r, "@%.*s defined twice", (int)n, s);
    char small[256];
    char *z = n < sizeof small ? small : malloc(n + 1);
    memcpy(z, s, n);
    z[n] = 0;
    g = LLVMAddGlobal(r->mod, t, z);
    if (z != small) free(z);
    size_t got;
    const char *nm = LLVMGetValueName2(g, &got);
    if (got != n || memcmp(nm, s, n) != 0) FAIL(r, "@%.*s's name taken", (int)n, s);
    table_put(&r->globals, s, n, g);
    LLVMSetLinkage(g, linkage);
    if (tls) LLVMSetThreadLocal(g, 1);
    if (unnamed) LLVMSetUnnamedAddress(g, LLVMGlobalUnnamedAddr);
    LLVMSetGlobalConstant(g, is_const);
    // (the initializer comes in the second pass)
    skip_line(r);
    return 1;
  }
  g = table_get(&r->globals, s, n);
  spaces(r);
  if (r->p < r->end && *r->p != ',' && *r->p != '\n') {
    LLVMValueRef init = constant(r, t);
    if (!init) FAIL(r, "a global's value");
    LLVMSetInitializer(g, init);
  } else if (linkage != LLVMExternalLinkage) {
    FAIL(r, "a global without a value");
  }
  if (!align_of(r, g)) FAIL(r, "a global's alignment");
  return line_end(r);
}

// The module, in two passes over the text: every function and global made
// (in the order written, as LLParser does), then each global's value and
// function's body.
static int module(R *r, const char *text, size_t len) {
  for (int pass = 0; pass < 2; pass++) {
    r->p = text;
    r->end = text + len;
    r->lineno = 1;
    while (r->p < r->end) {
      r->line = r->p;
      char c = *r->p;
      if (c == '\n' || c == ';') {
        skip_line(r);
        continue;
      }
      if (c == '@') {
        r->p++;
        const char *s;
        uint32_t n;
        if (!name(r, &s, &n) || !punct(r, '=')) FAIL(r, "a global's name");
        if (!global(r, s, n, pass == 0)) return 0;
        continue;
      }
      if (word(r, "declare")) {
        LLVMValueRef fn;
        if (pass == 0) {
          if (!header(r, 0, &fn, 1) || !line_end(r)) return 0;
        } else {
          skip_line(r);
        }
        continue;
      }
      if (word(r, "define")) {
        LLVMValueRef fn;
        if (pass == 0) {
          if (!header(r, 1, &fn, 1)) return 0;
          // (skip the body: to the line that is `}`)
          for (;;) {
            skip_line(r);
            if (r->p >= r->end) FAIL(r, "a function without its `}`");
            if (*r->p == '}') {
              skip_line(r);
              break;
            }
          }
        } else {
          if (!header(r, 1, &fn, 0) || !line_end(r) || !body(r, fn)) return 0;
        }
        continue;
      }
      FAIL(r, "a line this reader doesn't know");
    }
  }
  return 1;
}

// The module the IR text `ir` (`len` bytes) is, built in `ctx` with values
// unnamed (or named as written, with `names`); NULL when this reader can't
// read it (`why` says what it met, `line` where).
static LLVMModuleRef read_module(LLVMContextRef ctx, const char *ir, size_t len, int names, char *why, size_t whylen) {
  R *r = calloc(1, sizeof *r);
  r->ctx = ctx;
  r->names = names;
  r->mod = LLVMModuleCreateWithNameInContext("strata", ctx);
  r->b = LLVMCreateBuilderInContext(ctx);
  r->t_void = LLVMVoidTypeInContext(ctx);
  r->t_i1 = LLVMInt1TypeInContext(ctx);
  r->t_i8 = LLVMInt8TypeInContext(ctx);
  r->t_i16 = LLVMInt16TypeInContext(ctx);
  r->t_i32 = LLVMInt32TypeInContext(ctx);
  r->t_i64 = LLVMInt64TypeInContext(ctx);
  r->t_ptr = LLVMPointerTypeInContext(ctx, 0);
  r->t_double = LLVMDoubleTypeInContext(ctx);
  r->t_float = LLVMFloatTypeInContext(ctx);
  r->k_zeroext = LLVMGetEnumAttributeKindForName("zeroext", 7);
  r->k_signext = LLVMGetEnumAttributeKindForName("signext", 7);
  r->k_noreturn = LLVMGetEnumAttributeKindForName("noreturn", 8);
  r->k_noinline = LLVMGetEnumAttributeKindForName("noinline", 8);
  r->k_optnone = LLVMGetEnumAttributeKindForName("optnone", 7);
  r->k_alwaysinline = LLVMGetEnumAttributeKindForName("alwaysinline", 12);
  r->k_nounwind = LLVMGetEnumAttributeKindForName("nounwind", 8);
  table_init(&r->globals, 1 << 16);
  table_init(&r->locals, 1 << 12);
  table_init(&r->blocks, 1 << 10);
  int ok = module(r, ir, len);
  if (!ok) {
    const char *e = r->line ? r->line : ir;
    const char *nl = memchr(e, '\n', (size_t)(ir + len - e));
    int n = nl ? (int)(nl - e) : (int)(ir + len - e);
    if (n > 200) n = 200;
    snprintf(why, whylen, "line %d: %s: %.*s", r->lineno, r->err[0] ? r->err : "?", n, e);
    LLVMDisposeModule(r->mod);
  }
  LLVMDisposeBuilder(r->b);
  if (r->holes) LLVMDisposeModule(r->holes);
  free(r->globals.slots);
  free(r->locals.slots);
  free(r->blocks.slots);
  free(r->pending);
  LLVMModuleRef m = ok ? r->mod : NULL;
  free(r);
  return m;
}

// The module LLVM's own reader makes of `ir`: NULL (and `*msg`) when it
// rejects it.
static LLVMModuleRef parsed_module(LLVMContextRef ctx, const char *ir, size_t len, char **msg) {
  LLVMMemoryBufferRef buf = LLVMCreateMemoryBufferWithMemoryRangeCopy(ir, len, "strata");
  LLVMModuleRef mod;
  if (LLVMParseIRInContext(ctx, buf, &mod, msg)) return NULL;
  return mod;
}

// Whether STRATA_IR_CHECK is set (and not `0`).
static int checking(void) {
  const char *c = getenv("STRATA_IR_CHECK");
  return c && *c && strcmp(c, "0") != 0;
}

// compiler/codegen's IR text as a module in a new context of its own (both
// in `*ctx` and `*mod`): 0, else 1 with why LLVM rejected it in `*msg` (an
// LLVM message, LLVMDisposeMessage it). Read by this file's reader, or by
// LLVM's when it can't (STRATA_DEBUG says why); with STRATA_IR_CHECK, by
// both, compared.
int32_t toolchain_module(const char *ir, LLVMContextRef *ctx, LLVMModuleRef *mod, char **msg) {
  size_t len = strlen(ir);
  *msg = NULL;
  timings_begin("LLVM: parse IR");
  char why[512];
  why[0] = 0;
  int check = checking();
  LLVMContextRef c = LLVMContextCreate();
  if (!check) LLVMContextSetDiscardValueNames(c, 1);
  LLVMModuleRef m = getenv("STRATA_IR_LLPARSER") ? NULL : read_module(c, ir, len, check, why, sizeof why);
  if (m && check) {
    LLVMContextRef c2 = LLVMContextCreate();
    char *msg2 = NULL;
    LLVMModuleRef m2 = parsed_module(c2, ir, len, &msg2);
    if (m2) {
      LLVMSetModuleIdentifier(m2, "strata", 6);
      LLVMSetSourceFileName(m2, "strata", 6);
      char *a = LLVMPrintModuleToString(m), *b = LLVMPrintModuleToString(m2);
      if (strcmp(a, b) != 0) {
        const char *dir = getenv("TMPDIR") ? getenv("TMPDIR") : "/tmp";
        char pa[1024], pb[1024];
        snprintf(pa, sizeof pa, "%s/strata-ir-read.ll", dir);
        snprintf(pb, sizeof pb, "%s/strata-ir-llparser.ll", dir);
        FILE *fa = fopen(pa, "w"), *fb = fopen(pb, "w");
        if (fa) fputs(a, fa), fclose(fa);
        if (fb) fputs(b, fb), fclose(fb);
        fprintf(stderr, "STRATA_IR_CHECK: the modules differ: %s, %s\n", pa, pb);
      }
      LLVMDisposeMessage(a);
      LLVMDisposeMessage(b);
      LLVMDisposeModule(m2);
    }
    LLVMDisposeMessage(msg2);
    LLVMContextDispose(c2);
  }
  if (!m) {
    if (why[0] && (check || getenv("STRATA_DEBUG"))) fprintf(stderr, "note: LLVM reads the IR itself: %s\n", why);
    // (a context of its own: LLParser needs the values' names)
    LLVMContextDispose(c);
    c = LLVMContextCreate();
    m = parsed_module(c, ir, len, msg);
    if (!m) {
      timings_end("LLVM: parse IR");
      LLVMContextDispose(c);
      return 1;
    }
  }
  timings_end("LLVM: parse IR");
  *ctx = c;
  *mod = m;
  return 0;
}
