// The toolchain built into the compiler: LLVM IR (compiler/codegen's text)
// to an object file, and object files to an executable, in this process, with
// the LLVM and lld linked into the compiler's executable. No C compiler or
// linker has to be installed where `strata build` runs.
//
// C++ only for lld, which has no C API; the rest is LLVM's C API. Called
// from Strata through `extern c` (compiler/toolchain).

#include <llvm-c/Core.h>
#include <llvm-c/IRReader.h>
#include <llvm-c/Target.h>
#include <llvm-c/TargetMachine.h>
#include <llvm-c/Transforms/PassBuilder.h>

#include <CommonCrypto/CommonDigest.h>
#include <copyfile.h>
#include <sys/clonefile.h>
#include <sys/stat.h>
#include <unistd.h>

#include <algorithm>
#include <atomic>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <string_view>
#include <thread>
#include <unordered_map>
#include <vector>

// lld's Mach-O driver (lld/Common/Driver.h), declared here: building the
// compiler then needs only liblld.a (tools/build-lld.sh), and none of LLVM's
// C++ headers, which want C++17 (the package's C is compiled with the C
// compiler's default). `ArrayRef` as LLVM lays it out (a pointer and a
// length, passed by value); the streams only by reference.
namespace llvm {
class raw_ostream;
class raw_fd_ostream;
template <typename T> struct ArrayRef {
  const T *data;
  size_t length;
};
raw_fd_ostream &outs();
raw_fd_ostream &errs();
} // namespace llvm
namespace lld {
namespace macho {
bool link(llvm::ArrayRef<const char *> args, llvm::raw_ostream &stdoutOS, llvm::raw_ostream &stderrOS,
          bool exitEarly, bool disableOutput);
}
struct CommonLinkerContext {
  static void destroy();
};
} // namespace lld

static std::string last_error;

static int32_t fail(const std::string &m) {
  last_error = m;
  return 1;
}

extern "C" {

// Profiling (c/jit.c): every function defined keeps its frame pointer.
void toolchain_frame_pointers(LLVMModuleRef mod);
int32_t toolchain_profiling(void);

// `--timings` (c/driver.c): where a phase begins and ends.
int32_t timings_begin(const char *name);
int32_t timings_end(const char *name);

// Why the last `toolchain_object` or `toolchain_link` failed ("" when it didn't).
const char *toolchain_error(void) { return last_error.c_str(); }

// What objects are made for: Apple silicon Macs from macOS 13 on, any of
// them (the CPU clang picks for this target), whichever Mac builds them, so
// that an executable, and the precompiled pieces a release ships, run on
// every one. `toolchain_link` passes the same version to lld.
static const char *TRIPLE = "arm64-apple-macosx13.0.0";
static const char *CPU = "apple-m1";

const char *toolchain_triple(void) { return TRIPLE; }

// Compile IR text to an object file at `out`, optimized at `opt` (0-3: the
// passes of -O<opt>, then code generated at that level). 0 when it is
// written, else 1 (`toolchain_error` says why).
int32_t toolchain_object(const char *ir, const char *out, int32_t opt) {
  last_error.clear();
  LLVMInitializeAArch64TargetInfo();
  LLVMInitializeAArch64Target();
  LLVMInitializeAArch64TargetMC();
  LLVMInitializeAArch64AsmPrinter();
  LLVMInitializeAArch64AsmParser();
  LLVMContextRef ctx = LLVMContextCreate();
  LLVMMemoryBufferRef buf = LLVMCreateMemoryBufferWithMemoryRangeCopy(ir, strlen(ir), "strata");
  LLVMModuleRef mod;
  char *msg = nullptr;
  timings_begin("LLVM: parse IR");
  LLVMBool rejected = LLVMParseIRInContext(ctx, buf, &mod, &msg);
  timings_end("LLVM: parse IR");
  if (rejected) {
    std::string m = std::string("LLVM rejected the generated IR: ") + (msg ? msg : "");
    LLVMDisposeMessage(msg);
    LLVMContextDispose(ctx);
    return fail(m);
  }
  const char *triple = TRIPLE;
  LLVMTargetRef target;
  if (LLVMGetTargetFromTriple(triple, &target, &msg)) {
    std::string m = std::string("no target for ") + triple + ": " + (msg ? msg : "");
    LLVMDisposeMessage(msg);
    LLVMDisposeModule(mod);
    LLVMContextDispose(ctx);
    return fail(m);
  }
  LLVMSetTarget(mod, triple);
  toolchain_frame_pointers(mod);
  LLVMCodeGenOptLevel level = opt <= 0 ? LLVMCodeGenLevelNone
                              : opt == 1 ? LLVMCodeGenLevelLess
                              : opt == 2 ? LLVMCodeGenLevelDefault
                                         : LLVMCodeGenLevelAggressive;
  // position-independent, as every macOS executable
  LLVMTargetMachineRef tm =
      LLVMCreateTargetMachine(target, triple, CPU, "", level, LLVMRelocPIC, LLVMCodeModelDefault);
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
      r = fail(std::string("LLVM's optimizer failed: ") + m);
      LLVMDisposeErrorMessage(m);
    }
  }
  timings_begin("LLVM: machine code");
  if (!r && LLVMTargetMachineEmitToFile(tm, mod, out, LLVMObjectFile, &msg)) {
    r = fail(std::string("can't write ") + out + ": " + (msg ? msg : ""));
    LLVMDisposeMessage(msg);
  }
  timings_end("LLVM: machine code");
  LLVMDisposeTargetMachine(tm);
  LLVMDisposeModule(mod);
  LLVMContextDispose(ctx);
  return r;
}

} // extern "C"

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

namespace {

// One top-level entity of the IR.
struct Item {
  enum Kind { Common, Global, Declare, Define } kind;
  std::string_view text; // its lines, the last newline included
  std::string name;      // a global's, declaration's or function's (`@` left out, quotes kept)
  bool local = false;    // private or internal: copied into each part that reaches it
  std::vector<int> refs; // the items it names (indices into the items)
  // where it names a local item: [start, end) in `text`, the `@` included
  std::vector<std::pair<size_t, size_t>> spans;
};

bool name_char(char c) {
  return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '-' ||
         c == '$' || c == '.' || c == '_';
}

// The name at `p` (just after an `@`): a quoted name with its quotes, else
// the run of name characters. Its length in `*n`.
std::string_view name_at(std::string_view t, size_t p, size_t *n) {
  if (p < t.size() && t[p] == '"') {
    size_t q = t.find('"', p + 1);
    if (q == std::string_view::npos) q = t.size() - 1;
    *n = q + 1 - p;
  } else {
    size_t q = p;
    while (q < t.size() && name_char(t[q])) q++;
    *n = q - p;
  }
  return t.substr(p, *n);
}

// Each `@name` in `t` (outside string constants and comments): `f(name,
// start, end)`, the `@` at `start`.
template <typename F> void each_ref(std::string_view t, F f) {
  size_t i = 0;
  while (i < t.size()) {
    char c = t[i];
    if (c == ';') {
      while (i < t.size() && t[i] != '\n') i++;
    } else if (c == '"') {
      // a string constant (`c"..."`, `!"..."`): its quotes are escaped (\22)
      size_t q = t.find('"', i + 1);
      i = q == std::string_view::npos ? t.size() : q + 1;
    } else if (c == '%' && i + 1 < t.size() && t[i + 1] == '"') {
      size_t q = t.find('"', i + 2);
      i = q == std::string_view::npos ? t.size() : q + 1;
    } else if (c == '@') {
      size_t n;
      std::string_view nm = name_at(t, i + 1, &n);
      if (n) f(nm, i, i + 1 + n);
      i += 1 + n;
    } else {
      i++;
    }
  }
}

// The declaration of the function whose `define` line is `header`: its
// return type, name and parameters' types, without their names or the
// function's attributes.
std::string declaration_of(std::string_view header) {
  // `define RET @NAME(TYPE %a, TYPE %b) ATTRS {`
  size_t at = header.find('@');
  size_t n;
  name_at(header, at + 1, &n);
  size_t open = at + 1 + n; // the `(`
  std::string out = "declare ";
  out += header.substr(7, open - 7);
  int depth = 0;
  std::string params;
  for (size_t i = open; i < header.size(); i++) {
    char c = header[i];
    if (c == '(' || c == '[' || c == '{' || c == '<') depth++;
    if (c == ')' || c == ']' || c == '}' || c == '>') depth--;
    if (depth == 0) break;
    params += c;
  }
  // each ` %name` dropped
  for (size_t k = 0; k < params.size(); k++) {
    if (params[k] == ' ' && k + 1 < params.size() && params[k + 1] == '%') {
      k++;
      if (k + 1 < params.size() && params[k + 1] == '"') {
        size_t q = params.find('"', k + 2);
        k = q == std::string::npos ? params.size() : q;
      } else {
        while (k + 1 < params.size() && name_char(params[k + 1])) k++;
      }
      continue;
    }
    out += params[k];
  }
  out += ")\n";
  return out;
}

bool starts(std::string_view t, const char *p) { return t.substr(0, strlen(p)) == p; }

// `ir` read into its items and its functions cut into parts' texts. A part
// ends after a function once it holds at least `min_bytes`, at a function
// whose name hashes to 0 modulo 8, or at `max_bytes`: where parts end
// depends only on the functions around, not on the rest of the program, so
// an edit changes the parts of what it changed. A function of another part
// of at most `import_bytes` that a part's functions call is copied into it
// `available_externally` (to inline, as ThinLTO imports them; compiled only
// in its own part). `false` when the IR has what can't be split (a global
// defined for other modules to use).
bool split_ir(std::string_view ir, size_t min_bytes, size_t max_bytes, size_t import_bytes,
              std::vector<std::string> &out) {
  std::vector<Item> items;
  size_t i = 0;
  while (i < ir.size()) {
    size_t e = ir.find('\n', i);
    e = e == std::string_view::npos ? ir.size() : e + 1;
    std::string_view line = ir.substr(i, e - i);
    Item it{Item::Common, line, ""};
    if (starts(line, "define ")) {
      // to the line `}`
      size_t j = e;
      while (j < ir.size()) {
        size_t k = ir.find('\n', j);
        k = k == std::string_view::npos ? ir.size() : k + 1;
        std::string_view l = ir.substr(j, k - j);
        j = k;
        if (l == "}\n" || l == "}") break;
      }
      e = j;
      it.kind = Item::Define;
      it.text = ir.substr(i, e - i);
      size_t at = line.find('@');
      size_t n;
      it.name = std::string(name_at(line, at + 1, &n));
      it.local = starts(line, "define internal ") || starts(line, "define private ");
    } else if (starts(line, "declare ")) {
      it.kind = Item::Declare;
      size_t at = line.find('@');
      size_t n;
      it.name = std::string(name_at(line, at + 1, &n));
    } else if (starts(line, "@")) {
      it.kind = Item::Global;
      size_t n;
      it.name = std::string(name_at(line, 1, &n));
      std::string_view rest = line.substr(1 + n);
      it.local = starts(rest, " = private ") || starts(rest, " = internal ");
      if (!it.local && !starts(rest, " = external ")) return false;
    } else if (starts(line, ";") || line == "\n") {
      // comments (counts that change with the program) and blank lines
      i = e;
      continue;
    }
    items.push_back(it);
    i = e;
  }
  // names to items: a definition before a declaration
  std::unordered_map<std::string, int> by_name;
  for (int k = 0; k < (int)items.size(); k++) {
    const Item &it = items[k];
    if (it.kind == Item::Common) continue;
    auto f = by_name.find(it.name);
    if (f == by_name.end() || (items[f->second].kind == Item::Declare && it.kind != Item::Declare))
      by_name[it.name] = k;
  }
  for (Item &it : items) {
    if (it.kind != Item::Define && it.kind != Item::Global) continue;
    // (its own name is not a reference)
    size_t skip = it.kind == Item::Global ? 1 + it.name.size() : it.text.find('@') + 1 + it.name.size();
    each_ref(it.text, [&](std::string_view nm, size_t from, size_t to) {
      if (from < skip) return;
      auto f = by_name.find(std::string(nm));
      if (f == by_name.end()) return;
      it.refs.push_back(f->second);
      if (items[f->second].local) it.spans.push_back({from, to});
    });
  }
  std::vector<std::vector<int>> groups;
  std::vector<int> cur;
  size_t bytes = 0;
  for (int k = 0; k < (int)items.size(); k++) {
    const Item &it = items[k];
    if (it.kind != Item::Define || it.local) continue;
    cur.push_back(k);
    bytes += it.text.size();
    uint64_t h = 14695981039346656037ull;
    for (char c : it.name) h = (h ^ (unsigned char)c) * 1099511628211ull;
    if ((bytes >= min_bytes && (h & 7) == 0) || bytes >= max_bytes) {
      groups.push_back(cur);
      cur.clear();
      bytes = 0;
    }
  }
  if (!cur.empty()) groups.push_back(cur);
  std::string common;
  for (const Item &it : items)
    if (it.kind == Item::Common) common += it.text;
  std::vector<char> seen(items.size(), 0), own(items.size(), 0);
  std::vector<int> number(items.size(), -1);
  for (const std::vector<int> &g : groups) {
    // its functions, then the local items they reach (transitively), in the
    // order first named, and the small functions they call; the other names
    // declared
    std::vector<int> order, outside, imported, queue(g.begin(), g.end());
    for (int k : g) seen[k] = own[k] = 1;
    for (size_t q = 0; q < queue.size(); q++) {
      bool may_import = import_bytes > 0 && own[queue[q]];
      for (int r : items[queue[q]].refs) {
        if (seen[r]) continue;
        seen[r] = 1;
        const Item &it = items[r];
        if (it.local) {
          number[r] = (int)order.size();
          order.push_back(r);
          queue.push_back(r);
        } else if (may_import && it.kind == Item::Define && it.text.size() <= import_bytes &&
                   it.name != "main") {
          imported.push_back(r);
          queue.push_back(r);
        } else {
          outside.push_back(r);
        }
      }
    }
    auto emit = [&](std::string &s, int k) {
      const Item &it = items[k];
      size_t last = 0;
      if (it.local) {
        // its own name, renamed too
        size_t at = it.kind == Item::Global ? 0 : it.text.find('@');
        size_t n;
        name_at(it.text, at + 1, &n);
        s += it.text.substr(0, at);
        s += "@.l" + std::to_string(number[k]);
        last = at + 1 + n;
      }
      for (auto [from, to] : it.spans) {
        int r = by_name[std::string(it.text.substr(from + 1, to - from - 1))];
        s += it.text.substr(last, from - last);
        s += "@.l" + std::to_string(number[r]);
        last = to;
      }
      s += it.text.substr(last);
    };
    std::string s = common;
    std::sort(outside.begin(), outside.end());
    for (int r : outside) {
      const Item &it = items[r];
      if (it.kind == Item::Define)
        s += declaration_of(it.text.substr(0, it.text.find('\n')));
      else
        s += it.text; // a declaration, or an external global
    }
    for (int r : order) emit(s, r);
    for (int r : imported) {
      s += "define available_externally ";
      size_t at = s.size();
      emit(s, r);
      s.erase(at, 7); // its `define `
    }
    for (int k : g) emit(s, k);
    out.push_back(std::move(s));
    for (int k : g) own[k] = 0;
    for (int k : queue) seen[k] = 0;
    for (int k : outside) seen[k] = 0;
    for (int k : order) number[k] = -1;
  }
  return true;
}

std::string hex_sha256(const std::string &prefix, const std::string &text) {
  CC_SHA256_CTX c;
  CC_SHA256_Init(&c);
  CC_SHA256_Update(&c, prefix.data(), (CC_LONG)prefix.size());
  CC_SHA256_Update(&c, text.data(), (CC_LONG)text.size());
  unsigned char d[32];
  CC_SHA256_Final(d, &c);
  static const char *hx = "0123456789abcdef";
  std::string h;
  for (unsigned char b : d) {
    h += hx[b >> 4];
    h += hx[b & 15];
  }
  return h;
}

// One part compiled to `out`, as `toolchain_object` compiles, in a context
// and with a target machine of its own: "" when written, else why not.
std::string compile_part(const std::string &ir, const std::string &out, int32_t opt) {
  LLVMContextRef ctx = LLVMContextCreate();
  LLVMMemoryBufferRef buf = LLVMCreateMemoryBufferWithMemoryRange(ir.c_str(), ir.size(), "strata", 1);
  LLVMModuleRef mod;
  char *msg = nullptr;
  if (LLVMParseIRInContext(ctx, buf, &mod, &msg)) {
    std::string m = std::string("LLVM rejected the generated IR: ") + (msg ? msg : "");
    LLVMDisposeMessage(msg);
    LLVMContextDispose(ctx);
    return m;
  }
  LLVMTargetRef target;
  if (LLVMGetTargetFromTriple(TRIPLE, &target, &msg)) {
    std::string m = std::string("no target for ") + TRIPLE + ": " + (msg ? msg : "");
    LLVMDisposeMessage(msg);
    LLVMDisposeModule(mod);
    LLVMContextDispose(ctx);
    return m;
  }
  LLVMSetTarget(mod, TRIPLE);
  toolchain_frame_pointers(mod);
  LLVMCodeGenOptLevel level = opt <= 0 ? LLVMCodeGenLevelNone
                              : opt == 1 ? LLVMCodeGenLevelLess
                              : opt == 2 ? LLVMCodeGenLevelDefault
                                         : LLVMCodeGenLevelAggressive;
  LLVMTargetMachineRef tm =
      LLVMCreateTargetMachine(target, TRIPLE, CPU, "", level, LLVMRelocPIC, LLVMCodeModelDefault);
  LLVMTargetDataRef td = LLVMCreateTargetDataLayout(tm);
  char *layout = LLVMCopyStringRepOfTargetData(td);
  LLVMSetDataLayout(mod, layout);
  LLVMDisposeMessage(layout);
  LLVMDisposeTargetData(td);
  std::string r;
  if (opt > 0) {
    char passes[32];
    snprintf(passes, sizeof passes, "default<O%d>", opt > 3 ? 3 : opt);
    LLVMPassBuilderOptionsRef po = LLVMCreatePassBuilderOptions();
    LLVMErrorRef e = LLVMRunPasses(mod, passes, tm, po);
    LLVMDisposePassBuilderOptions(po);
    if (e) {
      char *m = LLVMGetErrorMessage(e);
      r = std::string("LLVM's optimizer failed: ") + m;
      LLVMDisposeErrorMessage(m);
    }
  }
  if (r.empty() && LLVMTargetMachineEmitToFile(tm, mod, out.c_str(), LLVMObjectFile, &msg)) {
    r = std::string("can't write ") + out + ": " + (msg ? msg : "");
    LLVMDisposeMessage(msg);
  }
  LLVMDisposeTargetMachine(tm);
  LLVMDisposeModule(mod);
  LLVMContextDispose(ctx);
  return r;
}

// (the largest function a part copies from another to inline: its text,
// about 30 lines; `STRATA_IMPORT_BYTES` to try others)
size_t IMPORT_BYTES =
    getenv("STRATA_IMPORT_BYTES") ? strtoul(getenv("STRATA_IMPORT_BYTES"), 0, 10) : 2048;

std::string objects_list;
int64_t objects_compiled = 0;

void make_dirs(const std::string &p) {
  for (size_t k = 1; k < p.size(); k++)
    if (p[k] == '/') mkdir(p.substr(0, k).c_str(), 0755);
  mkdir(p.c_str(), 0755);
}

} // namespace

extern "C" {

// The objects the last `toolchain_objects` made, in order, each after a
// newline.
const char *toolchain_objects_list(void) { return objects_list.c_str(); }

// The SHA-256, as 64 hex digits, of `what` and of the bytes of the files
// `paths` (each after a newline), each framed by its length (a missing file
// counts as `-1`): the key of what is made of those files.
const char *toolchain_files_key(const char *what, const char *paths) {
  static std::string key;
  std::string all = std::to_string(strlen(what)) + ":" + what;
  std::string_view ps(paths);
  size_t i = 0;
  while (i < ps.size()) {
    size_t e = ps.find('\n', i);
    if (e == std::string_view::npos) e = ps.size();
    std::string p(ps.substr(i, e - i));
    i = e + 1;
    if (p.empty()) continue;
    FILE *f = fopen(p.c_str(), "rb");
    if (!f) {
      all += "-1:";
      continue;
    }
    std::string bytes;
    char buf[65536];
    size_t n;
    while ((n = fread(buf, 1, sizeof buf, f)) > 0) bytes.append(buf, n);
    fclose(f);
    all += std::to_string(bytes.size()) + ":" + bytes;
  }
  key = hex_sha256("", all);
  return key.c_str();
}

// The SHA-256 (64 hex digits) of a link's arguments `args` (each after a
// newline), with each one's size and time of change that names a file or a
// folder (`-LDIR` too, for the libraries `-lNAME` finds there): what a
// linked executable is kept in the cache under. An object of the cache is
// named by its contents already; a library outside it (LLVM's) is told by
// its size and time.
const char *toolchain_link_key(const char *args) {
  static std::string key;
  std::string all;
  std::string_view as(args);
  size_t i = 0;
  while (i < as.size()) {
    size_t e = as.find('\n', i);
    if (e == std::string_view::npos) e = as.size();
    std::string a(as.substr(i, e - i));
    i = e + 1;
    all += std::to_string(a.size()) + ":" + a;
    std::string p = a.rfind("-L", 0) == 0 ? a.substr(2) : a;
    struct stat st;
    if (!p.empty() && p[0] == '/' && stat(p.c_str(), &st) == 0)
      all += " " + std::to_string((long long)st.st_size) + " " + std::to_string((long long)st.st_mtimespec.tv_sec) +
             "." + std::to_string((long long)st.st_mtimespec.tv_nsec);
    all += "\n";
  }
  key = hex_sha256("strata link 1\n", all);
  return key.c_str();
}

// The file `from` copied to `to` (replaced): a clone where the file system
// makes them (APFS: no bytes copied), else copied. 0 when done.
int32_t toolchain_clone(const char *from, const char *to) {
  unlink(to);
  if (clonefile(from, to, 0) == 0) return 0;
  return copyfile(from, to, nullptr, COPYFILE_ALL) == 0 ? 0 : 1;
}

// How many parts the last `toolchain_objects` compiled (the others' objects
// were in the cache).
int64_t toolchain_objects_compiled(void) { return objects_compiled; }

// The IR `ir` compiled, optimized at `opt` (0-3), to objects: in parts, by
// `threads` threads (0: one a core), each part's object kept in the folder
// `cache` under its key, or (`cache` "") written as `scratch`/part.N.o.
// How many objects (`toolchain_objects_list` names them), or -1
// (`toolchain_error` says why).
int32_t toolchain_objects(const char *ir, const char *scratch, const char *cache, int32_t opt,
                          int32_t threads) {
  last_error.clear();
  objects_list.clear();
  objects_compiled = 0;
  LLVMInitializeAArch64TargetInfo();
  LLVMInitializeAArch64Target();
  LLVMInitializeAArch64TargetMC();
  LLVMInitializeAArch64AsmPrinter();
  LLVMInitializeAArch64AsmParser();
  std::string_view text(ir);
  std::vector<std::string> parts;
  const char *whole = getenv("STRATA_LLVM_PARTS");
  bool one = whole && strcmp(whole, "0") == 0;
  unsigned major = 0, minor = 0, patch = 0;
  LLVMGetVersion(&major, &minor, &patch);
  char what[160];
  // (`--profile` keeps frame pointers: other objects)
  snprintf(what, sizeof what, "strata object 1\n%s %s O%d LLVM %u.%u.%u%s\n", TRIPLE, CPU, (int)opt,
           major, minor, patch, toolchain_profiling() ? " profiling" : "");
  std::string dir = cache;
  // the objects of an IR made before, listed under the key of the whole
  // text (`lists/KEY`): neither cut into parts nor each part's key worked
  // out again, when every one is still there
  std::string list;
  if (!dir.empty()) {
    std::string key = hex_sha256(std::string(what) + (one ? "list whole\n" : "list\n"), std::string(text));
    list = dir + "/lists/" + key;
    if (FILE *f = fopen(list.c_str(), "r")) {
      std::string got;
      char b[4096];
      size_t n;
      while ((n = fread(b, 1, sizeof b, f)) > 0) got.append(b, n);
      fclose(f);
      int32_t count = 0;
      bool all = !got.empty();
      size_t at = 0;
      while (all && at < got.size()) {
        size_t e = got.find('\n', at);
        if (e == std::string::npos) e = got.size();
        struct stat st;
        if (stat(got.substr(at, e - at).c_str(), &st) != 0) all = false;
        count++;
        at = e + 1;
      }
      if (all) {
        objects_list = got;
        return count;
      }
    }
  }
  // (optimized, larger parts: more of what a function calls is beside it,
  // to inline; `STRATA_LLVM_PARTS=0`: the whole module as one part, to
  // compare)
  timings_begin("LLVM: split IR");
  if (one ||
      !split_ir(text, opt > 0 ? 512 * 1024 : 128 * 1024, 4 * 1024 * 1024, opt > 0 ? IMPORT_BYTES : 0,
                parts)) {
    parts.clear();
    parts.push_back(std::string(text));
  }
  timings_end("LLVM: split IR");
  if (!dir.empty()) make_dirs(dir);
  std::vector<std::string> outs(parts.size()), errs(parts.size());
  std::vector<int> work;
  for (size_t k = 0; k < parts.size(); k++) {
    if (dir.empty()) {
      outs[k] = std::string(scratch) + "/part." + std::to_string(k) + ".o";
      work.push_back((int)k);
    } else {
      std::string key = hex_sha256(what, parts[k]);
      outs[k] = dir + "/" + key.substr(0, 2) + "/" + key + ".o";
      struct stat st;
      if (stat(outs[k].c_str(), &st) != 0) work.push_back((int)k);
    }
  }
  objects_compiled = (int64_t)work.size();
  // the largest parts first, so that no thread is left with a long one at the end
  std::stable_sort(work.begin(), work.end(),
                   [&](int a, int b) { return parts[a].size() > parts[b].size(); });
  unsigned n = threads > 0 ? (unsigned)threads : std::thread::hardware_concurrency();
  if (n == 0) n = 1;
  if (n > work.size()) n = (unsigned)work.size();
  timings_begin("LLVM: parts (parallel)");
  std::atomic<size_t> next{0};
  auto worker = [&]() {
    for (;;) {
      size_t w = next.fetch_add(1);
      if (w >= work.size()) return;
      int k = work[w];
      if (dir.empty()) {
        errs[k] = compile_part(parts[k], outs[k], opt);
      } else {
        // written beside it, then renamed: a part is in the cache whole or not at all
        mkdir(outs[k].substr(0, outs[k].rfind('/')).c_str(), 0755);
        std::string tmp = outs[k] + ".tmp-" + std::to_string(getpid()) + "-" + std::to_string(k);
        errs[k] = compile_part(parts[k], tmp, opt);
        if (errs[k].empty() && rename(tmp.c_str(), outs[k].c_str()) != 0) errs[k] = "can't write " + outs[k];
        if (!errs[k].empty()) unlink(tmp.c_str());
      }
    }
  };
  std::vector<std::thread> pool;
  for (unsigned t = 1; t < n; t++) pool.emplace_back(worker);
  if (n > 0) worker();
  for (std::thread &t : pool) t.join();
  timings_end("LLVM: parts (parallel)");
  for (size_t k = 0; k < parts.size(); k++)
    if (!errs[k].empty()) return fail(errs[k]) * -1;
  for (size_t k = 0; k < parts.size(); k++) objects_list += outs[k] + "\n";
  if (!list.empty()) {
    // (written beside it, then renamed: whole or not at all)
    make_dirs(dir + "/lists");
    std::string tmp = list + ".tmp-" + std::to_string(getpid());
    if (FILE *f = fopen(tmp.c_str(), "w")) {
      bool ok = fwrite(objects_list.data(), 1, objects_list.size(), f) == objects_list.size();
      if (fclose(f) == 0 && ok) rename(tmp.c_str(), list.c_str());
      else unlink(tmp.c_str());
    }
  }
  return (int32_t)parts.size();
}

// Link with lld's Mach-O linker (ld64.lld), given its arguments, each
// after a newline (the first is the linker's name). 0 when it linked, else
// 1 (lld has written why on standard error).
int32_t toolchain_link(const char *args) {
  last_error.clear();
  std::vector<std::string> parts;
  std::string cur;
  for (const char *p = args;; p++) {
    if (*p == '\n' || *p == 0) {
      parts.push_back(cur);
      cur.clear();
      if (*p == 0) break;
    } else {
      cur += *p;
    }
  }
  std::vector<const char *> argv;
  for (auto &s : parts) argv.push_back(s.c_str());
  // lld writes what it has to say (errors, warnings) on the process's
  // standard error itself: raw_fd_ostream's raw_ostream is its first base
  llvm::raw_ostream &out = reinterpret_cast<llvm::raw_ostream &>(llvm::outs());
  llvm::raw_ostream &err = reinterpret_cast<llvm::raw_ostream &>(llvm::errs());
  timings_begin("link (lld)");
  bool ok = lld::macho::link({argv.data(), argv.size()}, out, err, false, false);
  lld::CommonLinkerContext::destroy();
  timings_end("link (lld)");
  if (!ok) return fail("the linker failed");
  return 0;
}

} // extern "C"

#include <limits.h>
#include <mach-o/dyld.h>
#include <stdlib.h>

extern "C" {

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

} // extern "C"
