#!/bin/bash
# Compare the `strata` command written in Strata (the compiled compiler
# package) with the Rust one, command by command: the same arguments, in
# the same folder, the same stdout, stderr and exit code.
#
#   tools/compare-driver.sh OURS [RUST] < cases
#
# Each case is a line `DIR<TAB>ARGS...` (DIR relative to the repository;
# ARGS split on spaces); `#` lines are comments. RUST defaults to
# target/release/strata. Each command runs under a 300 s limit, each side
# with a cache of its own (CACHE, default a new folder: `rust/` and `ours/`),
# so that what one verified isn't the other's `(cached)`. Prints each
# case's verdict, the differences, and a summary; exit 1 when one differs.
set -u
cd "$(dirname "$0")/.."
root=$(pwd)
ours=$1
rust=${2:-$root/target/release/strata}
scratch=$(mktemp -d)
cache=${CACHE:-$scratch/cache}
limit() { perl -e 'alarm 300; exec @ARGV' "$@"; }
# what only one of them says about itself, not the command: Rust building
# its compiler package, or falling back to llvm.rs
plain() { grep -v -e '^building the Strata compiler' -e 'using llvm.rs$' "$1" || true; }
same=0; differ=0
while IFS=$'\t' read -r dir args; do
  case "$dir" in ''|'#'*) continue ;; esac
  for side in rust ours; do
    exe=$rust; [ $side = ours ] && exe=$ours
    (cd "$root/$dir" && STRATA_HOME=$root STRATA_CACHE=$cache/$side limit "$exe" $args > "$scratch/$side.out" 2> "$scratch/$side.err" < /dev/null; echo $? > "$scratch/$side.code")
    plain "$scratch/$side.err" > "$scratch/$side.err2"
  done
  if cmp -s "$scratch/rust.out" "$scratch/ours.out" && cmp -s "$scratch/rust.err2" "$scratch/ours.err2" && cmp -s "$scratch/rust.code" "$scratch/ours.code"; then
    same=$((same + 1)); echo "same     $dir: strata $args"
  else
    differ=$((differ + 1)); echo "DIFFERS  $dir: strata $args (exit $(cat "$scratch/rust.code") rust, $(cat "$scratch/ours.code") ours)"
    diff "$scratch/rust.out" "$scratch/ours.out" | head -10 | sed 's/^/    stdout /'
    diff "$scratch/rust.err2" "$scratch/ours.err2" | head -10 | sed 's/^/    stderr /'
  fi
done
rm -rf "$scratch"
echo "$same the same, $differ differ"
[ $differ = 0 ]
