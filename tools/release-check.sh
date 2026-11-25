#!/bin/sh
# Check a release archive (tools/release.sh) as a Mac without developer
# tools would run it: unpacked in a fresh folder, HOME a fresh folder, no
# STRATA_* variables, and a PATH of /usr/bin and /bin without clang, cc, ld,
# xcrun or z3. Runs `strata --version`, hello on the JIT, wc built into an
# executable (in-process objects, lld, the libSystem stub) and run, and
# `verify` on a script (the shipped z3); then the commands a user types, in
# a folder of their own (not the home): `run`, `build` (and the executable
# run), `check`, `verify` and `test`, none saying `unreadable` or `error`.
# Then the shipped cache (lib/strata/cache): a program reaching what it
# holds (std's comptime values) reads them from it, writing none of them
# into its own cache; and the first-run timings of run hello, build wc,
# verify math.strata and test letters.strata (it reaches the shipped
# entries), each with a fresh own cache, with and without the shipped one.
#
#   tools/release-check.sh [ARCHIVE]
set -e
cd "$(dirname "$0")/.."
REPO=$(pwd)
ARCHIVE=${1:-target/release-dist/strata-macos-arm64.tar.gz}
ARCHIVE=$(cd "$(dirname "$ARCHIVE")" && pwd)/$(basename "$ARCHIVE")
T=$(mktemp -d)
mkdir -p "$T/inst" "$T/home" "$T/bin"
(cd "$T/inst" && tar xzf "$ARCHIVE")
for d in /usr/bin /bin; do
  for f in "$d"/*; do
    n=$(basename "$f")
    case $n in clang | clang++ | cc | c++ | gcc | g++ | ld | xcrun | as | cpp | z3 | llvm-* | lldb) continue ;; esac
    [ -e "$T/bin/$n" ] || ln -s "$f" "$T/bin/$n"
  done
done
R=$T/inst/strata
L=$R/lib/strata
H=$T/home
clean() { env -i PATH="$T/bin" HOME="$H" TMPDIR="$H" STRATA_CC=/nonexistent "$@"; }
clean sh -c 'command -v clang || command -v ld || command -v z3' && { echo "release-check: the PATH still has developer tools" >&2; exit 1; }
cp -R examples/hello examples/wc "$H/"
cp examples/math.strata examples/books.strata "$H/"
# (a program of the user's reaching std.unicode's graphemes, whose tables
# are comptime values the release ships)
cat > "$H/letters.st" <<'EOF'
/// The letters (graphemes) of each word of `text`, counted.
fn letter_counts(text: String) -> List[Int] {
  split(text, " ").map(w -> len(w.graphemes()))
}

fn main() {
  print(f"{letter_counts("Zoë and Chloë")}")
}

test "letter counts" {
  assert letter_counts("Zoë and Chloë") == [3, 3, 5]
}
EOF
fail() { echo "release-check: $*" >&2; exit 1; }
clean "$R/bin/strata" --version | grep -q '^strata ' || fail "--version"
# (lib/strata/VERSION is the compiler's own version: release.sh asks it)
[ "strata $(cat "$L/VERSION")" = "$(clean "$R/bin/strata" --version)" ] || fail "lib/strata/VERSION isn't bin/strata's --version"
# (its identity, read from lib/strata/IDENTITY instead of hashing it: still
# bin/strata's after unpacking)
[ "$(cat "$L/IDENTITY")" = "$(shasum -a 256 "$R/bin/strata" | cut -d' ' -f1) $(stat -f '%z %m' "$R/bin/strata")" ] || fail "lib/strata/IDENTITY isn't bin/strata's"
# (the compiler reads core/ and the prelude from the current folder: its home)
cd "$L"
[ "$(clean "$R/bin/strata" jit --load "$L/std/libstd.dylib" "$H/hello/main.strata")" = "hello, world!" ] || fail "hello on the JIT"
clean "$R/bin/strata" codegen --precompiled "$L/std/names" "$H/wc" > "$H/wc.ll"
! grep -q '^error' "$H/wc.ll" || fail "codegen wc"
clean "$R/bin/strata" object "$H/wc.ll" "$H/wc.o" || fail "object"
clean "$R/bin/strata" link -arch arm64 -platform_version macos 13.0 13.0 -Z -L "$L" -lSystem -o "$H/wc.exe" \
  "$H/wc.o" "$L/runtime-lto.o" "$L/std/std.o" || fail "link"
printf 'a b c\nd e\n' > "$H/in.txt"
clean "$H/wc.exe" "$H/in.txt" | grep -q '^2	5	10	' || fail "wc built"
clean "$R/bin/strata" verify "$H/math.strata" > "$H/verify.out" 2>&1 || true
grep -q 'proven' "$H/verify.out" || { cat "$H/verify.out" >&2; fail "verify"; }
# the commands a user types, in a folder of their own: the compiler finds its
# home (lib/strata) from where it is, whatever the current folder
cd "$H"
said() { ! grep -q -e 'unreadable' -e '^error' "$H/u.out" || { cat "$H/u.out" >&2; fail "$*: says unreadable or error"; }; }
user() { clean "$R/bin/strata" "$@" > "$H/u.out" 2>&1 || { cat "$H/u.out" >&2; fail "$*"; }; said "$@"; }
user run hello
grep -q '^hello, world!$' "$H/u.out" || fail "run hello"
user build wc -o wcx
clean "$H/wcx" "$H/in.txt" | grep -q '^2	5	10	' || fail "wcx (built by \`strata build wc\`)"
# a first build (a fresh own cache) compiles the program alone: the runtime
# (lib/strata/runtime-lto.o) and std (std/std.o) are the release's, never
# compiled on the user's machine (no C compiler, no core.rt, no std library)
clean STRATA_CACHE="$H/first-build" "$R/bin/strata" build wc -o wcf --no-verify --timings > "$H/u.out" 2>&1 \
  || { cat "$H/u.out" >&2; fail "first build wc"; }
! grep -q -e 'C compiler' -e 'std library' "$H/u.out" || { cat "$H/u.out" >&2; fail "a first build compiled C or std's library"; }
made=$(cd "$H/first-build" && ls -d core-rt std-library objects/runtime-* 2>/dev/null || true)
[ -z "$made" ] || fail "a first build made what the release ships: $made"
user --file wc run -- "$H/in.txt"
grep -q '^2	5	10	' "$H/u.out" || fail "--file wc run"
user check math.strata
grep -q '^ok: ' "$H/u.out" || fail "check math.strata"
# (math.strata holds functions verify refutes on purpose: exit 1)
clean "$R/bin/strata" verify math.strata > "$H/u.out" 2>&1 || true
said verify math.strata
grep -q 'proven' "$H/u.out" || { cat "$H/u.out" >&2; fail "verify math.strata"; }
user --file math.strata test
grep -q ' passed, 0 failed$' "$H/u.out" || fail "--file math.strata test"
# std.sqlite: the system's libsqlite3, loaded for the JIT and linked (through
# lib/strata/libsqlite3.tbd) into the executable
user run books.strata
grep -q '^left to read: 2$' "$H/u.out" || fail "run books.strata"
user --file books.strata test
grep -q '^2 passed, 0 failed$' "$H/u.out" || fail "--file books.strata test"
user build books.strata -o booksx
clean "$H/booksx" | grep -q '^left to read: 2$' || fail "booksx (built by \`strata build books.strata\`)"
# the shipped cache: what it holds is read from it, never computed again
# into the own cache (HOME's)
[ -n "$(find "$L/cache" -type f)" ] || fail "lib/strata/cache is empty"
clean "$R/bin/strata" --file letters.strata edit < "$H/letters.st" > "$H/u.out" 2>&1 || { cat "$H/u.out" >&2; fail "edit letters.strata"; }
user --file letters.strata test
grep -q '^1 passed, 0 failed$' "$H/u.out" || fail "--file letters.strata test"
own=$H/.cache/strata
[ -d "$own" ] || fail "no own cache at $own"
again=$(cd "$L/cache" && find . -type f | sed 's|^\./||' | while read -r rel; do if [ -e "$own/$rel" ]; then echo "$rel"; fi; done)
[ -z "$again" ] || fail "computed again despite the shipped cache: $again"
# (nor was bin/strata hashed: its identity came from lib/strata/IDENTITY)
[ ! -d "$own/identity" ] || fail "bin/strata was hashed (own cache's identity/): lib/strata/IDENTITY not read"
# first-run timings, a fresh own cache each (the best of two): with the
# shipped cache, and without it (STRATA_SHIPPED_CACHE naming no folder)
ms() { perl -MTime::HiRes=time -e 'printf "%d", time * 1000'; }
n=0
# (`timed N CMD...`: the milliseconds CMD takes, with own cache first-N)
timed() {
  own=$H/first-$1
  shift
  t0=$(ms)
  clean STRATA_CACHE="$own" $SHIPPED "$R/bin/strata" "$@" > "$H/t.out" 2>&1 || true
  t1=$(ms)
  echo $((t1 - t0))
}
best() { [ "$1" -lt "$2" ] && echo "$1" || echo "$2"; }
echo "release-check: first runs (ms), fresh own cache:   shipped cache / none"
for c in "run hello" "build wc -o wct" "verify math.strata" "--file letters.strata test"; do
  SHIPPED=STRATA_SHIPPED_CACHE=/nonexistent
  w1=$(timed $((n + 1)) $c)
  SHIPPED=
  s1=$(timed $((n + 2)) $c)
  SHIPPED=STRATA_SHIPPED_CACHE=/nonexistent
  w2=$(timed $((n + 3)) $c)
  SHIPPED=
  s2=$(timed $((n + 4)) $c)
  n=$((n + 4))
  printf 'release-check:   %-28s %8s / %s\n' "$c" "$(best $s1 $s2)" "$(best $w1 $w2)"
done
# (and without it, letters.strata computes what it ships (its first run
# without, first-(n-3)): the check above was not vacuous)
missing=$(cd "$L/cache" && find comptime -type f | while read -r rel; do if [ ! -e "$H/first-$((n - 3))/$rel" ]; then echo "$rel"; fi; done)
[ -z "$missing" ] || fail "letters.strata, without the shipped cache, didn't compute: $missing"
echo "release-check: $ARCHIVE works without developer tools ($T)"
