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

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
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
  if (LLVMParseIRInContext(ctx, buf, &mod, &msg)) {
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
    LLVMErrorRef e = LLVMRunPasses(mod, passes, tm, po);
    LLVMDisposePassBuilderOptions(po);
    if (e) {
      char *m = LLVMGetErrorMessage(e);
      r = fail(std::string("LLVM's optimizer failed: ") + m);
      LLVMDisposeErrorMessage(m);
    }
  }
  if (!r && LLVMTargetMachineEmitToFile(tm, mod, out, LLVMObjectFile, &msg)) {
    r = fail(std::string("can't write ") + out + ": " + (msg ? msg : ""));
    LLVMDisposeMessage(msg);
  }
  LLVMDisposeTargetMachine(tm);
  LLVMDisposeModule(mod);
  LLVMContextDispose(ctx);
  return r;
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
  bool ok = lld::macho::link({argv.data(), argv.size()}, out, err, false, false);
  lld::CommonLinkerContext::destroy();
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
