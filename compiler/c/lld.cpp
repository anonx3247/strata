// lld's Mach-O linker for compiler/c/toolchain.c: the one piece of the
// toolchain in C++, as lld has no C API. Its driver (lld/Common/Driver.h)
// declared here by hand, so that building the compiler needs only liblld.a
// (tools/build-lld.sh), and neither LLVM's C++ headers (which want C++17)
// nor the C++ standard library's (tens of thousands of lines to compile).
// `ArrayRef` as LLVM lays it out (a pointer and a length, passed by value);
// the streams only by reference.

namespace llvm {
class raw_ostream;
class raw_fd_ostream;
template <typename T> struct ArrayRef {
  const T *data;
  decltype(sizeof 0) length;
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

// lld's Mach-O linker (ld64.lld) on `argv` (`argc` of them, the first the
// linker's name): 1 when it linked, else 0 (lld has written why on standard
// error itself: raw_fd_ostream's raw_ostream is its first base).
extern "C" int toolchain_lld(int argc, const char **argv) {
  llvm::raw_ostream &out = reinterpret_cast<llvm::raw_ostream &>(llvm::outs());
  llvm::raw_ostream &err = reinterpret_cast<llvm::raw_ostream &>(llvm::errs());
  bool ok = lld::macho::link({argv, (decltype(sizeof 0))argc}, out, err, false, false);
  lld::CommonLinkerContext::destroy();
  return ok ? 1 : 0;
}
