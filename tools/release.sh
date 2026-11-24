#!/bin/sh
# Assemble a release of Strata for macOS on Apple silicon:
# target/release-dist/strata-macos-arm64.tar.gz and its .sha256, holding
#
#   strata/bin/strata               the compiler (compiler/, compiled), LLVM and
#                                   lld linked in (no clang or linker needed)
#   strata/lib/strata/              what it finds relative to itself (its home)
#     VERSION
#     core/ std/                    core's and std's modules (sources)
#     src/llvm/prelude.strata       the prelude codegen compiles programs with
#     runtime.o runtime-core.o      the C runtime, without and with core.rt
#                                   (-DSTRATA_CORE_RT), precompiled
#     core_rt.o                     core.rt (the runtime written in Strata)
#     std/std.o std/libstd.dylib std/names
#                                   std's precompiled code: linked into
#                                   executables, loaded by the JIT
#     libSystem.tbd                 the stub executables are linked against
#                                   (no SDK on the user's machine)
#     z3                            the SMT solver `verify` runs
#     cache/                        the shipped, read-only cache (filled from
#                                   CACHE_FROM when given)
#
#   tools/release.sh                build everything (Homebrew's llvm, zstd, z3
#                                   and clang needed: on the machine making the
#                                   release, never on the user's)
#   COMPILER=EXE tools/release.sh   with a compiler executable already built
#   CACHE_FROM=DIR                  a populated cache to ship in lib/strata/cache
#   Z3=PATH                         the z3 binary to ship (default: z3 on PATH)
set -e
cd "$(dirname "$0")/.."
REPO=$(pwd)
VERSION=0.1.0
PLATFORM=macos-arm64
DIST=$REPO/target/release-dist
ROOT=$DIST/strata
HOME_DIR=$ROOT/lib/strata
rm -rf "$ROOT"
mkdir -p "$ROOT/bin" "$HOME_DIR/std" "$HOME_DIR/core" "$HOME_DIR/src/llvm" "$HOME_DIR/cache"
step() { echo "release: $*" >&2; }

# the compiler, LLVM and lld statically linked in
if [ -z "$COMPILER" ]; then
  [ -f target/lld/liblld.a ] || tools/build-lld.sh
  step "building the compiler (strata build compiler)"
  STRATA_BACKEND=rust target/release/strata build --rust --no-verify compiler -o "$DIST/compiler.tmp" >"$DIST/compiler-build.log" 2>&1 \
    || { tail -20 "$DIST/compiler-build.log" >&2; exit 1; }
  COMPILER=$DIST/compiler.tmp
fi
cp "$COMPILER" "$ROOT/bin/strata"
strip -x "$ROOT/bin/strata" 2>/dev/null && codesign -f -s - "$ROOT/bin/strata" 2>/dev/null || true
S=$ROOT/bin/strata
# what it may link: macOS's own libraries only
if otool -L "$S" | tail -n +2 | grep -v -E '^\s*/usr/lib/|^\s*/System/' >&2; then
  echo "release: bin/strata links libraries a Mac doesn't have (above)" >&2
  exit 1
fi

# its home: core's and std's sources, the prelude
step "core, std, prelude"
cp core/*.strata core/strata.toml "$HOME_DIR/core/"
cp std/*.strata std/strata.toml "$HOME_DIR/std/"
cp src/llvm/prelude.strata "$HOME_DIR/src/llvm/"
echo "$VERSION" > "$HOME_DIR/VERSION"

# the C runtime, precompiled for macOS 13 on (as the compiler's objects are)
step "runtime.c"
CC=${CC:-clang}
$CC -O2 -mmacosx-version-min=13.0 -Wno-deprecated-declarations -c src/llvm/runtime.c -o "$HOME_DIR/runtime.o"
$CC -O2 -mmacosx-version-min=13.0 -Wno-deprecated-declarations -DSTRATA_CORE_RT -c src/llvm/runtime.c -o "$HOME_DIR/runtime-core.o"

# core.rt and std, compiled by the compiler itself (its codegen, its LLVM)
step "core.rt"
"$S" codegen --core-rt > "$DIST/core_rt.ll"
! grep -q '^error' "$DIST/core_rt.ll" || { grep '^error' "$DIST/core_rt.ll" >&2; exit 1; }
"$S" object "$DIST/core_rt.ll" "$HOME_DIR/core_rt.o" -O 2
step "std (about 20 s)"
"$S" codegen --library > "$DIST/std.ll"
! grep -q '^error' "$DIST/std.ll" || { grep '^error' "$DIST/std.ll" >&2; exit 1; }
"$S" object "$DIST/std.ll" "$HOME_DIR/std/std.o" -O 2
sed -n 's/^; precompiled //p' "$DIST/std.ll" > "$HOME_DIR/std/names"

# libSystem's stub: everything the shipped objects call must be in it
cp compiler/c/libSystem.tbd "$HOME_DIR/libSystem.tbd"
tools/libsystem-tbd.sh "$HOME_DIR/runtime.o" "$HOME_DIR/runtime-core.o" "$HOME_DIR/core_rt.o" "$HOME_DIR/std/std.o" > "$DIST/libSystem.tbd"
missing=$(tr ',' '\n' < "$DIST/libSystem.tbd" | sed -n "s/.*'\(.*\)'.*/\1/p" | sort -u | comm -23 - "$(tr ',' '\n' < compiler/c/libSystem.tbd | sed -n "s/.*'\(.*\)'.*/\1/p" | sort -u > "$DIST/have"; echo "$DIST/have")")
if [ -n "$missing" ]; then
  echo "release: compiler/c/libSystem.tbd lacks $(echo $missing): run tools/libsystem-tbd.sh (see it)" >&2
  exit 1
fi
"$S" link -arch arm64 -platform_version macos 13.0 13.0 -Z -L "$HOME_DIR" -lSystem -dylib -undefined dynamic_lookup \
  -install_name @rpath/libstd.dylib -o "$HOME_DIR/std/libstd.dylib" "$HOME_DIR/std/std.o"

# z3: Homebrew's links only libc++ and libSystem
step "z3"
Z3=${Z3:-$(command -v z3)}
Z3=$(cd "$(dirname "$Z3")" && pwd -P)/$(basename "$Z3")
Z3=$(readlink -f "$Z3")
if otool -L "$Z3" | tail -n +2 | grep -v -E '^\s*/usr/lib/|^\s*/System/' >&2; then
  echo "release: $Z3 links libraries a Mac doesn't have (above)" >&2
  exit 1
fi
cp "$Z3" "$HOME_DIR/z3"
chmod 755 "$HOME_DIR/z3"

# the shipped cache
if [ -n "$CACHE_FROM" ]; then
  step "cache from $CACHE_FROM"
  cp -R "$CACHE_FROM"/. "$HOME_DIR/cache/"
fi

rm -f "$DIST"/*.ll "$DIST/have" "$DIST/libSystem.tbd" "$DIST/compiler.tmp"
ARCHIVE=strata-$PLATFORM.tar.gz
(cd "$DIST" && COPYFILE_DISABLE=1 tar czf "$ARCHIVE" strata && shasum -a 256 "$ARCHIVE" > "$ARCHIVE.sha256")
step "$DIST/$ARCHIVE ($(du -h "$DIST/$ARCHIVE" | cut -f1); bin/strata $(du -h "$S" | cut -f1))"
