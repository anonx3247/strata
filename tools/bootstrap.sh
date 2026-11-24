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
# target/seed. Stage 1 = the seed's build of compiler/ (by compiler/codegen);
# each next stage is the previous one's build of it, until two in a row are
# the same bytes (a fixed point: 2 builds with a seed of these sources, up
# to 4 with an older one). That stage is target/strata, and
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
# (the libSystem stub programs are linked against: compiler/c/libSystem.tbd)
ln -s "$root/compiler" "$seed_home/compiler"
ln -s "$root/runtime/prelude.strata" "$seed_home/src/llvm/prelude.strata"
ln -s "$root/runtime/runtime.c" "$seed_home/src/llvm/runtime.c"
# Stage A is the seed's build of these sources; each next stage is the
# previous one's build of them, until two in a row are the same bytes (a
# fixed point). A seed built from these sources gives A = B. An older seed
# (the last release) can take two more: A runs these sources' code but on
# the seed's runtime and code generation, so B can differ from what B itself
# builds (C), and C = D. More than four builds is a failure.
rm -rf target/stages
mkdir -p target/stages/1
STRATA_HOME=$seed_home "$seed" build "$root/compiler" --no-verify --release -o target/stages/1/strata
export STRATA_HOME=$root
n=1
while :; do
  next=$((n + 1))
  mkdir -p "target/stages/$next"
  "target/stages/$n/strata" build compiler --no-verify --release -o "target/stages/$next/strata"
  if cmp -s "target/stages/$n/strata" "target/stages/$next/strata"; then
    break
  fi
  if [ "$next" -ge 4 ]; then
    echo "not a fixed point: stages $n and $next (target/stages/) still differ after $next builds" >&2
    exit 1
  fi
  echo "stage $next differs from stage $n (built by it): building stage $((next + 1))" >&2
  n=$next
done
mv "target/stages/$next/strata" target/strata
echo "$(shasum -a 256 target/strata | cut -d' ' -f1) $(stat -f '%z %m' target/strata)" > target/strata.identity
rm -rf target/stages "$seed_home"
echo "strata built by itself (target/strata, a fixed point)"
