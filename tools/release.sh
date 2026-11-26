#!/bin/sh
# Assemble a release of Strata for macOS on Apple silicon:
# target/release-dist/strata-macos-arm64.tar.gz and its .sha256, holding
#
#   strata/bin/strata               the compiler (compiler/, compiled), LLVM and
#                                   lld linked in (no clang or linker needed)
#   strata/share/man/               man1/strata.1, man3/std.MODULE.3: the
#                                   man pages `strata docgen --man` writes
#   strata/lib/strata/              what it finds relative to itself (its home)
#     VERSION
#     core/ std/                    core's and std's modules (sources)
#     docs/                         docs/*.md, the pages `strata doc` shows
#                                   (docs/std/ is left out: `strata doc
#                                   std.MODULE` makes it from std's code)
#     runtime/prelude.strata        the prelude codegen compiles programs with
#     runtime/runtime.c, wasm.c, wasm.h  the C runtime, for WebAssembly modules
#     runtime-lto.o                 the runtime every executable links: the C
#                                   runtime (-DSTRATA_CORE_RT) and core.rt
#                                   (the runtime written in Strata) in one
#                                   module, optimized together at -O3 (LTO):
#                                   a user's build never compiles it
#     std/std.o std/libstd.dylib std/names
#                                   std's precompiled code: linked into
#                                   executables, loaded by the JIT
#     libSystem.tbd                 the stub executables are linked against
#                                   (no SDK on the user's machine)
#     libsqlite3.tbd                the stub of macOS's libsqlite3, for the
#                                   executables that call std.sqlite (std.o
#                                   leaves that code out: it is compiled with
#                                   the programs using it)
#     z3                            the SMT solver `verify` runs
#     IDENTITY                      bin/strata's SHA-256, size and
#                                   modification time: its identity for the
#                                   caches' keys, read instead of hashing
#                                   86 MB each run (trusted while the size
#                                   and time are bin/strata's)
#     cache/                        the shipped, read-only cache: what any
#                                   program's first run would compute from
#                                   std, core and the compiler alone (std's
#                                   comptime values), made by bin/strata
#                                   itself (its keys start with the SHA-256
#                                   of its bytes: made last, and the binary
#                                   never changed after)
#
#   tools/release.sh                build everything (Homebrew's llvm, zstd, z3
#                                   and clang needed: on the machine making the
#                                   release, never on the user's): the compiler
#                                   is target/strata (tools/bootstrap.sh makes
#                                   it when it isn't there)
#   COMPILER=EXE tools/release.sh   with a compiler executable already built
#   CACHE_FROM=DIR                  a populated cache to ship in lib/strata/cache
#                                   too (made by the same bin/strata)
#   Z3=PATH                         the z3 binary to ship (default: z3 on PATH)
set -e
cd "$(dirname "$0")/.."
REPO=$(pwd)
PLATFORM=macos-arm64
DIST=$REPO/target/release-dist
ROOT=$DIST/strata
HOME_DIR=$ROOT/lib/strata
rm -rf "$ROOT"
mkdir -p "$ROOT/bin" "$HOME_DIR/std" "$HOME_DIR/core" "$HOME_DIR/runtime" "$HOME_DIR/cache" "$HOME_DIR/docs"
step() { echo "release: $*" >&2; }

# the compiler, LLVM and lld statically linked in
# (the compiler built by itself: target/strata, made by tools/bootstrap.sh)
if [ -z "$COMPILER" ]; then
  if [ ! -x target/strata ]; then
    step "building the compiler (tools/bootstrap.sh)"
    tools/bootstrap.sh >"$DIST/compiler-build.log" 2>&1 \
      || { tail -20 "$DIST/compiler-build.log" >&2; exit 1; }
  fi
  cp target/strata "$DIST/compiler.tmp"
  COMPILER=$DIST/compiler.tmp
fi
# the release's version: the compiler's own (`strata --version`, compiler/
# main's `version`), the one place it is written
VERSION=$("$COMPILER" --version | sed -n 's/^strata //p')
[ -n "$VERSION" ] || { echo "release: $COMPILER --version says no version" >&2; exit 1; }
cp "$COMPILER" "$ROOT/bin/strata"
strip -x "$ROOT/bin/strata" 2>/dev/null && codesign -f -s - "$ROOT/bin/strata" 2>/dev/null || true
S=$ROOT/bin/strata
# its identity (compiler/c/cache.c): written now, after strip and codesign,
# the last change to it (checked at the end)
identity() { echo "$(shasum -a 256 "$1" | cut -d' ' -f1) $(stat -f '%z %m' "$1")"; }
identity "$S" > "$HOME_DIR/IDENTITY"
# what it may link: macOS's own libraries only
if otool -L "$S" | tail -n +2 | grep -v -E '^\s*/usr/lib/|^\s*/System/' >&2; then
  echo "release: bin/strata links libraries a Mac doesn't have (above)" >&2
  exit 1
fi

# its home: core's and std's sources, the prelude
step "core, std, prelude"
cp core/*.strata core/strata.toml "$HOME_DIR/core/"
cp std/*.strata std/strata.toml "$HOME_DIR/std/"
cp docs/*.md "$HOME_DIR/docs/"
cp runtime/prelude.strata "$HOME_DIR/runtime/"
# (`strata build --target wasm32` compiles these for each module, with Homebrew's clang)
cp runtime/runtime.c runtime/wasm.c runtime/wasm.h "$HOME_DIR/runtime/"
echo "$VERSION" > "$HOME_DIR/VERSION"

# the man pages: `man strata`, `man std.io` (man looks in the share/man
# beside each bin/ on the PATH)
step "man pages"
STRATA_HOME="$REPO" "$S" --file std docgen --man -o "$ROOT/share/man" >&2

# the runtime, precompiled for macOS 13 on (as the compiler's objects are):
# runtime.c (clang's LTO bitcode) and core.rt (the compiler's codegen) linked
# into one module (llvm-link), optimized at -O3 and compiled by the
# compiler itself (its LLVM): calls between the two are inlined. Homebrew's
# clang and llvm-link, whose LLVM is the compiler's (the bitcode is read by
# the same version).
step "runtime (runtime.c and core.rt, -O3, LTO)"
LLVM_BIN=${LLVM_BIN:-$(brew --prefix llvm)/bin}
"$LLVM_BIN/clang" -O3 -flto -mmacosx-version-min=13.0 -Wno-deprecated-declarations -DSTRATA_CORE_RT \
  -c runtime/runtime.c -o "$DIST/runtime.bc"
"$S" codegen --core-rt > "$DIST/core_rt.ll"
! grep -q '^error' "$DIST/core_rt.ll" || { grep '^error' "$DIST/core_rt.ll" >&2; exit 1; }
# (it warns that core.rt's IR names no data layout: the object gets the
# target's)
"$LLVM_BIN/llvm-link" -S "$DIST/runtime.bc" "$DIST/core_rt.ll" -o "$DIST/runtime-lto.ll" 2>"$DIST/llvm-link.log" \
  || { cat "$DIST/llvm-link.log" >&2; exit 1; }
"$S" object "$DIST/runtime-lto.ll" "$HOME_DIR/runtime-lto.o" -O 3

# std, compiled by the compiler itself (its codegen, its LLVM)
step "std (about 20 s)"
"$S" codegen --library > "$DIST/std.ll"
! grep -q '^error' "$DIST/std.ll" || { grep '^error' "$DIST/std.ll" >&2; exit 1; }
"$S" object "$DIST/std.ll" "$HOME_DIR/std/std.o" -O 2
sed -n 's/^; precompiled //p' "$DIST/std.ll" > "$HOME_DIR/std/names"

# libSystem's stub: everything the shipped objects call must be in it
cp compiler/c/libSystem.tbd "$HOME_DIR/libSystem.tbd"
cp compiler/c/libsqlite3.tbd "$HOME_DIR/libsqlite3.tbd"
tools/libsystem-tbd.sh "$HOME_DIR/runtime-lto.o" "$HOME_DIR/std/std.o" > "$DIST/libSystem.tbd"
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

# the shipped cache: what a first run computes that doesn't depend on the
# program. Two warm-up programs with nothing of their own in common (both
# reach std.unicode's graphemes, whose tables are `comptime` values) each go
# through check, test, run, verify and build with an own cache of their own;
# an entry both wrote, the same path (its key) and the same bytes, was
# computed from std, core and the compiler alone: shipped. (Not the scratch
# folders: smt/ (solver inputs), run/ and build/ (one per process); nor
# identity/, an executable's identity by its path.)
step "the shipped cache (warm-up programs)"
W=$DIST/warm
rm -rf "$W"
mkdir -p "$W"
cat > "$W/a.st" <<'EOF'
/// How many letters (graphemes) `s` has.
fn letters(s: String) -> Int {
  len(s.graphemes())
}

fn main() {
  print(f"{letters("Zoë")}")
}

test "letters" {
  assert letters("Zoë") == 3
}
EOF
cat > "$W/b.st" <<'EOF'
/// The first letter (grapheme) of `s`, if any.
fn first_letter(s: String) -> Option[String] {
  s.graphemes().get(0)
}

fn main() {
  print(first_letter("ëa") ?? "")
}

test "first letter" {
  assert first_letter("ëa") == Some("ë")
}
EOF
# (`warm P CMD...`: CMD in a clean environment, with program P's own cache)
warm() { own=$W/own-$1; shift; (cd "$W" && env -i PATH=/usr/bin:/bin HOME="$W" TMPDIR="$W" STRATA_CACHE="$own" STRATA_SHIPPED_CACHE=/nonexistent "$@"); }
for p in a b; do
  warm $p "$S" --file "warm_$p.strata" edit < "$W/$p.st" > "$W/$p.log" 2>&1 || { cat "$W/$p.log" >&2; exit 1; }
  for cmd in check test run verify build; do
    out=
    [ $cmd = build ] && out="-o $W/x-$p"
    warm $p "$S" --file "warm_$p.strata" $cmd $out >> "$W/$p.log" 2>&1 \
      || { cat "$W/$p.log" >&2; echo "release: warm-up $p: $cmd failed" >&2; exit 1; }
  done
  (cd "$W/own-$p" && find . -type f | sed 's|^\./||' | grep -v -E '^(smt|run|build|identity)/' | sort) > "$W/$p.entries"
done
comm -12 "$W/a.entries" "$W/b.entries" | while read -r rel; do
  if cmp -s "$W/own-a/$rel" "$W/own-b/$rel"; then
    mkdir -p "$HOME_DIR/cache/$(dirname "$rel")"
    cp "$W/own-a/$rel" "$HOME_DIR/cache/$rel"
  fi
done
[ -n "$(ls "$HOME_DIR/cache")" ] || { echo "release: the warm-up programs left nothing to ship in lib/strata/cache ($W)" >&2; exit 1; }
if [ -n "$CACHE_FROM" ]; then
  step "cache from $CACHE_FROM"
  cp -R "$CACHE_FROM"/. "$HOME_DIR/cache/"
fi
[ "$(identity "$S")" = "$(cat "$HOME_DIR/IDENTITY")" ] || { echo "release: bin/strata changed after its cache was made (or its IDENTITY written)" >&2; exit 1; }
step "lib/strata/cache: $(find "$HOME_DIR/cache" -type f | wc -l | tr -d ' ') entries, $(du -sh "$HOME_DIR/cache" | cut -f1)"

rm -rf "$DIST"/*.ll "$DIST"/*.bc "$DIST/llvm-link.log" "$DIST/have" "$DIST/libSystem.tbd" "$DIST/compiler.tmp" "$W"
ARCHIVE=strata-$PLATFORM.tar.gz
(cd "$DIST" && COPYFILE_DISABLE=1 tar czf "$ARCHIVE" strata && shasum -a 256 "$ARCHIVE" > "$ARCHIVE.sha256")
step "$DIST/$ARCHIVE ($(du -h "$DIST/$ARCHIVE" | cut -f1); bin/strata $(du -h "$S" | cut -f1))"
