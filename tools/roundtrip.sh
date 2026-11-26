#!/bin/sh
# roundtrip.sh [--strata S] [PATH...]: checks the law parse(print(t)) == t
# for every stored tree. Each module file is printed (`view --proofs`), read
# back into a copy (`edit`), and the two files' trees compared
# (`header --tree`, in any order: `view` orders definitions its own way). PATH: .strata files or folders (default: the
# repository). A package's modules are read back into a copy of the package,
# so that their references resolve as in the package.
# Cost: one view and one edit per module; the edit checks the package.
set -u
strata=strata
if [ "${1:-}" = "--strata" ]; then strata=$2; shift 2; fi
case "$strata" in /*) ;; */*) strata="$PWD/$strata" ;; esac
root=$(cd "$(dirname "$0")/.." && pwd)
[ $# -eq 0 ] && set -- "$root"
tmp=$(mktemp -d "${TMPDIR:-/tmp}/roundtrip.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
fails=0
count=0

# the package folder holding file $1, or "" for a script
package_of() {
  d=$(dirname "$1")
  while [ "$d" != "/" ] && [ "$d" != "." ]; do
    [ -f "$d/strata.toml" ] && { echo "$d"; return; }
    d=$(dirname "$d")
  done
  echo ""
}

check_file() {
  f=$1
  count=$((count + 1))
  pkg=$(package_of "$f")
  if [ -z "$pkg" ]; then
    "$strata" --file "$f" view main --proofs > "$tmp/a.st" 2> "$tmp/err" || { echo "FAIL $f: view: $(head -1 "$tmp/err")"; fails=$((fails + 1)); return; }
    rm -f "$tmp/b.strata"
    "$strata" --file "$tmp/b.strata" edit < "$tmp/a.st" > "$tmp/err" 2>&1
    back="$tmp/b.strata"
  else
    rel=${f#"$pkg"/}
    module=${rel%.strata}
    copy="$tmp/pkg"
    if [ ! -d "$copy" ] || [ "$(cat "$tmp/pkg.from" 2>/dev/null)" != "$pkg" ]; then
      rm -rf "$copy"
      cp -cR "$pkg" "$copy" 2>/dev/null || cp -R "$pkg" "$copy"
      echo "$pkg" > "$tmp/pkg.from"
    fi
    "$strata" --file "$pkg" view "$module" --proofs > "$tmp/a.st" 2> "$tmp/err" || { echo "FAIL $f: view: $(head -1 "$tmp/err")"; fails=$((fails + 1)); return; }
    "$strata" --file "$copy" edit "$module" < "$tmp/a.st" > "$tmp/err" 2>&1
    back="$copy/$rel"
  fi
  "$strata" header "$f" --tree 2>&1 | sort > "$tmp/t1"
  "$strata" header "$back" --tree 2>&1 | sort > "$tmp/t2"
  if ! cmp -s "$tmp/t1" "$tmp/t2"; then
    echo "FAIL $f: the tree read back differs:"
    diff "$tmp/t1" "$tmp/t2" | head -6 | cut -c1-300
    fails=$((fails + 1))
  fi
}

for p in "$@"; do
  if [ -d "$p" ]; then
    find "$p" -name '*.strata' -not -path '*/target/*' -not -path '*/.claude/*' -not -path '*/.strata/*' | sort > "$tmp/list"
    while read -r f; do check_file "$f"; done < "$tmp/list"
  else
    check_file "$p"
  fi
done
echo "$count files, $fails differ"
[ "$fails" -eq 0 ]
