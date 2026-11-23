#!/bin/sh
# Build strata in two stages: first with the C runtime's versions of what
# core/rt.strata defines (stage 0), then, with that strata having compiled
# core.rt (`strata build-core`), with the runtime written in Strata (stage 1).
# Run it after changing core/rt.strata or core/mem.strata; a plain
# `cargo build` then keeps stage 1 while target/core_rt.ll matches them.
set -e
cd "$(dirname "$0")/.."
# stage 0: the C runtime's versions, whatever core_rt.ll is there
rm -f target/core_rt.ll
cargo build --release
target/release/strata build-core -o target/core_rt.ll.new
mv target/core_rt.ll.new target/core_rt.ll
cargo build --release
# the second stage compiles core.rt as the first did
target/release/strata build-core | cmp -s - target/core_rt.ll || { echo "stage 1 compiles core.rt differently from stage 0" >&2; exit 1; }
echo "strata built with core.rt (target/core_rt.ll)"
