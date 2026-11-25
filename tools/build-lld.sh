#!/bin/sh
# Build lld's Mach-O linker as one static library, target/lld/liblld.a, for
# the compiler's built-in linker (compiler/c/lld.cpp). Homebrew's lld
# ships only dylibs linked against libLLVM.dylib (161 MB, itself linked with
# Homebrew's z3 and zstd), which a release can't carry; this compiles lld's
# Common and MachO sources, from the llvm-project release matching Homebrew's
# LLVM, against Homebrew's LLVM headers. Needs: Homebrew's llvm (its
# llvm-tblgen and headers), clang++, curl. About 2 minutes.
set -e
cd "$(dirname "$0")/.."
LLVM=${LLVM:-/opt/homebrew/opt/llvm}
VERSION=$("$LLVM/bin/llvm-config" --version)
SHA256=d3b86ab29cfb4445a6652e5c2642ccc75e04e38084dde3701d323320f35a31da  # llvm-project-23.1.6.src.tar.xz
OUT=target/lld
SRC=$OUT/llvm-project-$VERSION.src
mkdir -p "$OUT"
if [ ! -d "$SRC/lld" ]; then
  tarball=$OUT/llvm-project-$VERSION.src.tar.xz
  [ -f "$tarball" ] || curl -fsSL -o "$tarball" "https://github.com/llvm/llvm-project/releases/download/llvmorg-$VERSION/llvm-project-$VERSION.src.tar.xz"
  echo "$SHA256  $tarball" | shasum -a 256 -c - >/dev/null || { echo "build-lld: $tarball isn't llvm-project $VERSION (checksum)" >&2; exit 1; }
  tar xf "$tarball" -C "$OUT" "llvm-project-$VERSION.src/lld" "llvm-project-$VERSION.src/libunwind/include"
fi
GEN=$OUT/gen
mkdir -p "$GEN/lld/Common" "$OUT/obj"
"$LLVM/bin/llvm-tblgen" -gen-opt-parser-defs -I "$LLVM/include" -o "$GEN/Options.inc" "$SRC/lld/MachO/Options.td"
echo "#define LLD_VERSION_STRING \"$VERSION\"" > "$GEN/lld/Common/Version.inc"
printf '#undef LLD_REVISION\n#undef LLD_REPOSITORY\n' > "$GEN/VCSVersion.inc"
# lld's driver initializes "all" targets (InitializeAllTargets): only
# AArch64 here, so that the compiler links one target's libraries, not 20
mkdir -p "$GEN/llvm/Config"
for d in Targets AsmPrinters AsmParsers Disassemblers TargetMCAs TargetExegesis; do
  sed -E '/^LLVM_[A-Z_]+\(/{/\(AArch64\)/!d;}' "$LLVM/include/llvm/Config/$d.def" > "$GEN/llvm/Config/$d.def"
done
CXX=${CXX:-clang++}
flags="-std=c++17 -O2 -fno-rtti -fno-exceptions -DNDEBUG -D__STDC_CONSTANT_MACROS -D__STDC_FORMAT_MACROS -D__STDC_LIMIT_MACROS
  -I$SRC/lld/include -I$SRC/lld/MachO -I$GEN -I$SRC/libunwind/include -I$LLVM/include"
# (DriverDispatcher, which names every driver, left out: the shim calls lld::macho::link)
srcs=$(cd "$SRC/lld" && ls Common/*.cpp MachO/*.cpp MachO/Arch/*.cpp | grep -v DriverDispatcher)
export CXX flags SRC OUT
# (8 at a time: each takes a few hundred MB)
echo "$srcs" | xargs -P 8 -n 1 sh -c '
  o=$OUT/obj/$(echo "$0" | tr / _ | sed "s/\.cpp\$/.o/")
  [ -f "$o" ] && [ ! "$SRC/lld/$0" -nt "$o" ] || $CXX $flags -c "$SRC/lld/$0" -o "$o"'
rm -f "$OUT/liblld.a"
ar rcs "$OUT/liblld.a" "$OUT"/obj/*.o
echo "built $OUT/liblld.a (lld $VERSION, Mach-O)"
