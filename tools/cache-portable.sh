#!/bin/sh
# tools/cache-portable.sh [COMPILER_EXE]
#
# Checks that the compiler's caches (compiler/cache) are keyed by content,
# so a cache populated on one machine is valid on another, as the release's
# shipped cache must be: a cache made at one path is copied to another path,
# the compiler executable is copied elsewhere (another path, another
# modification time), and the second run must hit the cache everywhere:
# every verdict `(cached)`, the compile-time value read back, not computed.
# Then the release layout: the cache in `<prefix>/lib/strata/cache` beside
# `<prefix>/bin/strata`, an empty own cache, which must stay without
# comptime values and verdicts (they are read from the shipped cache).
#
# COMPILER_EXE: the compiler built from compiler/ (default: built here with
# `strata build compiler --no-verify`). Run from the repository's root.
set -eu

root=$(pwd)
strata=${STRATA:-$root/target/strata}
tmp=$(mktemp -d "${TMPDIR:-/tmp}/cache-portable.XXXXXX")
trap 'rm -rf "$tmp"' EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }

mkdir -p "$tmp/a/bin" "$tmp/b/bin" "$tmp/b/lib/strata" "$tmp/prog"
if [ $# -ge 1 ]; then
  cp "$1" "$tmp/a/bin/strata"
else
  echo "building the compiler ..."
  "$strata" build compiler --no-verify -o "$tmp/a/bin/strata" > "$tmp/build.log" 2>&1 \
    || { cat "$tmp/build.log"; fail "building the compiler"; }
fi

# a program with a compile-time value that takes a while, and contracts
cat > "$tmp/prog/prog.st" <<'EOF'
fn squares(n: Int) -> Int {
  mut t := 0
  for i in 0..n {
    t += i * i
  }
  t
}

fn table() -> Int {
  comptime squares(4000000)
}

fn clamp(x: Int, lo: Int, hi: Int) -> Int
  requires lo <= hi
  ensures lo <= it && it <= hi {
  if x < lo {
    lo
  } else if x > hi {
    hi
  } else {
    x
  }
}

fn width(lo: Int, hi: Int) -> Natural
  requires lo <= hi {
  hi - lo
}

fn main() {
  print(to_string(table() + clamp(3, 0, 2) + width(1, 4)))
}
EOF
"$strata" --file "$tmp/prog/prog.strata" edit < "$tmp/prog/prog.st" > /dev/null

# run EXE CACHE OUT: `verify` of the program, its report in OUT, its time
# (seconds, with two decimals) printed
run() {
  start=$(perl -MTime::HiRes=time -e 'printf "%.3f", time')
  STRATA_HOME=$root STRATA_CACHE=$2 "$1" verify "$tmp/prog/prog.strata" > "$3" 2>&1 \
    || { cat "$3"; fail "verify with $1"; }
  perl -MTime::HiRes=time -e "printf '%.2f', time - $start"
}

entries() { find "$1" -type f -path "*/$2/*" 2>/dev/null | wc -l | tr -d ' '; }

# 1. a cold run populates cache c1 (no shipped cache: STRATA_SHIPPED_CACHE "")
export STRATA_SHIPPED_CACHE=""
t1=$(run "$tmp/a/bin/strata" "$tmp/c1" "$tmp/1.txt")
grep -q '(cached)' "$tmp/1.txt" && fail "the first run found cached verdicts"
[ "$(entries "$tmp/c1" comptime)" -ge 1 ] || fail "the first run kept no compile-time value"
echo "cold run, cache c1:                    ${t1} s"

# 2. the cache copied to another path, the executable to another path with
# another modification time
cp -R "$tmp/c1" "$tmp/c2"
sleep 1
cp "$tmp/a/bin/strata" "$tmp/b/bin/strata"
touch "$tmp/b/bin/strata"
[ "$(stat -f %m "$tmp/a/bin/strata" 2>/dev/null || stat -c %Y "$tmp/a/bin/strata")" \
  != "$(stat -f %m "$tmp/b/bin/strata" 2>/dev/null || stat -c %Y "$tmp/b/bin/strata")" ] \
  || fail "the copied executable has the same modification time"
rm -rf "$tmp/c1"
t2=$(run "$tmp/b/bin/strata" "$tmp/c2" "$tmp/2.txt")
n=$(grep -c . "$tmp/2.txt")
c=$(grep -c '(cached)' "$tmp/2.txt" || true)
[ "$n" = "$c" ] || { cat "$tmp/2.txt"; fail "copied cache: $c of $n verdicts cached"; }
echo "copied cache c2, moved executable:     ${t2} s ($c of $n verdicts cached)"

# 3. the release layout: the cache shipped beside bin/, read-only; the own
# cache empty and left without comptime values or verdicts
unset STRATA_SHIPPED_CACHE
cp -R "$tmp/c2" "$tmp/b/lib/strata/cache"
chmod -R a-w "$tmp/b/lib/strata/cache"
t3=$(run "$tmp/b/bin/strata" "$tmp/c3" "$tmp/3.txt")
chmod -R u+w "$tmp/b/lib/strata/cache"
c=$(grep -c '(cached)' "$tmp/3.txt" || true)
[ "$n" = "$c" ] || { cat "$tmp/3.txt"; fail "shipped cache: $c of $n verdicts cached"; }
[ "$(entries "$tmp/c3" comptime)" = 0 ] || fail "the compile-time value was computed again"
[ "$(entries "$tmp/c3" checks)" = 0 ] || fail "verdicts were written again"
echo "shipped cache, empty own cache:        ${t3} s ($c of $n verdicts cached, nothing recomputed)"

# 4. the program changed: its changed definition is checked again
sed 's/squares(4000000)/squares(4000001)/' "$tmp/prog/prog.st" \
  | "$strata" --file "$tmp/prog/prog.strata" edit > /dev/null
t4=$(run "$tmp/b/bin/strata" "$tmp/c3" "$tmp/4.txt")
grep -q '^. table .*(cached)' "$tmp/4.txt" && fail "a changed definition kept its verdict"
[ "$(entries "$tmp/c3" comptime)" -ge 1 ] || fail "the changed compile-time value was not computed"
echo "changed program:                       ${t4} s (recomputed what changed)"
echo "ok"
