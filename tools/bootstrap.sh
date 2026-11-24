#!/bin/sh
# Build strata: the `strata` command written in Strata (the compiler
# package, compiler/), building itself.
#
#   tools/bootstrap.sh                    seed: $STRATA_SEED, else `strata` on
#                                         PATH, else the latest release
#   STRATA_SEED=EXE tools/bootstrap.sh    seed: EXE (a strata built before)
#
# The seed is a `strata` built before: STRATA_SEED, else one installed on the
# PATH, else the latest release, downloaded (and its SHA-256 checked) into
# target/seed. Stage A = the seed's build of compiler/ (by compiler/codegen);
# stage B = stage A's build of it. A and B must be the same bytes (a fixed
# point); when the seed is an older strata (A and B differ), stage C = B's
# build of it must be B's bytes instead. B is target/strata, and
# target/strata.identity its SHA-256, size and modification time (its
# identity for the caches' keys, read instead of hashing it each run:
# compiler/c/cache.c).
#
# Needs LLVM 23, zstd and clang (Homebrew: `brew install llvm zstd`) and lld
# built from source into target/lld (tools/build-lld.sh): compiler/strata.toml
# links them into the compiler.
set -e
cd "$(dirname "$0")/.."
root=$(pwd)
[ -f target/lld/liblld.a ] || tools/build-lld.sh

if [ -n "$STRATA_SEED" ]; then
  seed=$STRATA_SEED
elif command -v strata >/dev/null 2>&1; then
  seed=$(command -v strata)
else
  url=https://github.com/anonx3247/strata/releases/latest/download/strata-macos-arm64.tar.gz
  echo "no strata to start from: downloading the latest release ($url)" >&2
  rm -rf target/seed
  mkdir -p target/seed
  curl -fsSL "$url" -o target/seed/strata.tar.gz
  curl -fsSL "$url.sha256" -o target/seed/strata.tar.gz.sha256
  want=$(cut -d' ' -f1 < target/seed/strata.tar.gz.sha256)
  got=$(shasum -a 256 target/seed/strata.tar.gz | cut -d' ' -f1)
  [ "$want" = "$got" ] || { echo "the release's SHA-256 is $got, not $want" >&2; exit 1; }
  tar xzf target/seed/strata.tar.gz -C target/seed
  seed=$root/target/seed/strata/bin/strata
fi
echo "seed: $seed" >&2

# (built where they are found: core/, std/ and the prelude are this
# repository's; the compiler package isn't verified here: `strata
# verify`. Each stage is named `strata`, in a folder of its own: the
# linker signs an executable under its file's name. `--release`: optimized,
# as builds are unoptimized by default.)
#
# The seed's home: this repository, as the seed's version lays it out (a
# strata from before runtime/ looked for the prelude and the C runtime in
# src/llvm/): a folder of links to core/, std/ and runtime/, under both names.
seed_home=$root/target/seed-home
rm -rf "$seed_home"
mkdir -p "$seed_home/src/llvm"
ln -s "$root/core" "$seed_home/core"
ln -s "$root/std" "$seed_home/std"
ln -s "$root/runtime" "$seed_home/runtime"
ln -s "$root/runtime/prelude.strata" "$seed_home/src/llvm/prelude.strata"
ln -s "$root/runtime/runtime.c" "$seed_home/src/llvm/runtime.c"
mkdir -p target/stage-a target/stage-b target/stage-c
STRATA_HOME=$seed_home "$seed" build "$root/compiler" --no-verify --release -o target/stage-a/strata
export STRATA_HOME=$root
target/stage-a/strata build compiler --no-verify --release -o target/stage-b/strata
if ! cmp -s target/stage-a/strata target/stage-b/strata; then
  # the seed compiles differently from these sources (an older strata, such
  # as the last release): B is these sources' own build, so B's build of
  # itself, C, must be B again
  echo "stage A (built by $seed) differs from stage B: building stage C" >&2
  target/stage-b/strata build compiler --no-verify --release -o target/stage-c/strata
  cmp -s target/stage-b/strata target/stage-c/strata || { echo "not a fixed point: target/stage-b/strata and target/stage-c/strata (built by it) differ" >&2; exit 1; }
fi
mv target/stage-b/strata target/strata
echo "$(shasum -a 256 target/strata | cut -d' ' -f1) $(stat -f '%z %m' target/strata)" > target/strata.identity
rm -rf target/stage-a target/stage-b target/stage-c "$seed_home"
echo "strata built by itself (target/strata, a fixed point)"
