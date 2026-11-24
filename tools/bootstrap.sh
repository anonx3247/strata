#!/bin/sh
# Build strata.
#
# 1. The Rust strata (until src/ goes), in two stages: first with the C
#    runtime's versions of what core/rt.strata defines (stage 0), then, with
#    that strata having compiled core.rt (`strata build-core`), with the
#    runtime written in Strata (stage 1). A plain `cargo build` then keeps
#    stage 1 while target/core_rt.ll matches core's runtime files.
# 2. The `strata` command written in Strata (the compiler package,
#    compiler/), building itself: seed = the compiler package built by the
#    Rust strata (or STRATA_SEED, a strata built before, which skips step 1);
#    stage A = the seed's build of compiler/ (by compiler/codegen); stage B =
#    stage A's build of it. A and B must be the same bytes (a fixed point);
#    B is target/strata.
set -e
cd "$(dirname "$0")/.."
root=$(pwd)
if [ -z "$STRATA_SEED" ]; then
  # stage 0: the C runtime's versions, whatever core_rt.ll is there
  rm -f target/core_rt.ll
  cargo build --release
  target/release/strata build-core -o target/core_rt.ll.new
  mv target/core_rt.ll.new target/core_rt.ll
  cargo build --release
  # the second stage compiles core.rt as the first did
  target/release/strata build-core | cmp -s - target/core_rt.ll || { echo "stage 1 compiles core.rt differently from stage 0" >&2; exit 1; }
  echo "strata built with core.rt (target/core_rt.ll)"
  STRATA_BACKEND=rust target/release/strata build --rust --no-verify compiler -o target/strata-seed
  seed=$root/target/strata-seed
else
  seed=$STRATA_SEED
fi
# (built where they are found: core/, std/ and the prelude are this
# repository's; the compiler package isn't verified here: `strata
# verify`. Each stage is named `strata`, in a folder of its own: the
# linker signs an executable under its file's name.)
export STRATA_HOME=$root
mkdir -p target/stage-a target/stage-b
"$seed" build compiler --no-verify -o target/stage-a/strata
target/stage-a/strata build compiler --no-verify -o target/stage-b/strata
cmp -s target/stage-a/strata target/stage-b/strata || { echo "not a fixed point: target/stage-a/strata (built by $seed) and target/stage-b/strata (built by it) differ" >&2; exit 1; }
mv target/stage-b/strata target/strata
rm -rf target/stage-a target/stage-b
echo "strata built by itself (target/strata, a fixed point)"
