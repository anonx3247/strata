#!/bin/sh
# Check that a build's bytes don't depend on what the cache held: build a
# package (default compiler/) with an empty cache, where its compile-time
# values are run by the JIT in the same process before the program is
# compiled, then again with only those values (and std's library, core.rt)
# kept, so that every object is compiled again in a process that ran no
# JIT. The two executables must be the same bytes (same file name: the
# linker signs an executable under it).
#
#   tools/reproducible.sh [PACKAGE] [BUILD FLAGS...]
#   STRATA=EXE tools/reproducible.sh        (default target/strata)
#
# About a minute for compiler/ with --release (the default flags). See
# `toolchain_selection` in compiler/c/jit.c for the kind of bug it finds.
set -e
cd "$(dirname "$0")/.."
root=$(pwd)
strata=${STRATA:-$root/target/strata}
package=${1:-compiler}
[ $# -gt 0 ] && shift
[ $# -gt 0 ] || set -- --no-verify --release
work=$(mktemp -d "${TMPDIR:-/tmp}/strata-reproducible.XXXXXX")
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/cold" "$work/warm"
export STRATA_HOME=$root STRATA_CACHE=$work/cache
"$strata" build "$package" "$@" -o "$work/cold/program" > "$work/cold.log" 2>&1 || { cat "$work/cold.log"; exit 1; }
# (what the second build may keep: compile-time values, std's library and
# core.rt; the program's objects, IR and executables go)
rm -rf "$work/cache/objects" "$work/cache/build" "$work/cache/ir" "$work/cache/proofs-lines"
"$strata" build "$package" "$@" -o "$work/warm/program" > "$work/warm.log" 2>&1 || { cat "$work/warm.log"; exit 1; }
if cmp -s "$work/cold/program" "$work/warm/program"; then
  echo "reproducible: $package builds the same bytes with and without its compile-time values cached"
else
  echo "NOT reproducible: $package's executables differ ($(cmp -l "$work/cold/program" "$work/warm/program" | wc -l | tr -d ' ') bytes)" >&2
  size -m "$work/cold/program" | grep __text >&2
  size -m "$work/warm/program" | grep __text >&2
  exit 1
fi
