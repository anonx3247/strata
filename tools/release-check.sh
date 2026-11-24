#!/bin/sh
# Check a release archive (tools/release.sh) as a Mac without developer
# tools would run it: unpacked in a fresh folder, HOME a fresh folder, no
# STRATA_* variables, and a PATH of /usr/bin and /bin without clang, cc, ld,
# xcrun or z3. Runs `strata --version`, hello on the JIT, wc built into an
# executable (in-process objects, lld, the libSystem stub) and run, and
# `verify` on a script (the shipped z3); then the commands a user types, in
# a folder of their own (not the home): `run`, `build` (and the executable
# run), `check`, `verify` and `test`, none saying `unreadable` or `error`.
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
cp examples/math.strata "$H/"
fail() { echo "release-check: $*" >&2; exit 1; }
clean "$R/bin/strata" --version | grep -q '^strata ' || fail "--version"
# (the compiler reads core/ and the prelude from the current folder: its home)
cd "$L"
[ "$(clean "$R/bin/strata" jit --load "$L/std/libstd.dylib" "$H/hello/main.strata")" = "hello, world!" ] || fail "hello on the JIT"
clean "$R/bin/strata" codegen --precompiled "$L/std/names" "$H/wc" > "$H/wc.ll"
! grep -q '^error' "$H/wc.ll" || fail "codegen wc"
clean "$R/bin/strata" object "$H/wc.ll" "$H/wc.o" || fail "object"
clean "$R/bin/strata" link -arch arm64 -platform_version macos 13.0 13.0 -Z -L "$L" -lSystem -o "$H/wc.exe" \
  "$H/wc.o" "$L/core_rt.o" "$L/runtime-core.o" "$L/std/std.o" || fail "link"
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
echo "release-check: $ARCHIVE works without developer tools ($T)"
