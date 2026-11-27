# drive.sh STRATA: runs srv.strata with `strata run --live` and edits it while it runs, each
# edit once the run has said what it did with the last one. Prints the
# program's output, then its notes (the latencies as N).
S=$1
wait_for() { # FILE TEXT: waits (up to 60 s) until FILE holds TEXT
  i=0
  while ! grep -qF -- "$2" "$1" 2>/dev/null; do
    i=$((i + 1))
    if [ $i -gt 600 ]; then echo "timed out waiting for: $2"; return 1; fi
    sleep 0.1
  done
}
edit() { # N FILE [ARGS]: live edit N
  n=$1 f=$2
  shift 2
  "$S" --file srv.strata edit --as-given "$@" < "$f" > /dev/null 2>&1
  wait_for err.txt "live edit $n"
}
"$S" --file srv.strata run --live > out.txt 2> err.txt &
pid=$!
wait_for out.txt hello
edit 1 world.st
edit 2 unknown.st
edit 3 helper.st
edit 4 louder.st
edit 5 signature.st
edit 6 type.st
edit 7 restored.st
edit 8 counterexample.st
edit 9 stop.st --rm doubled --rm Small
wait $pid
echo "exit $?"
cat out.txt
sed -E 's/[0-9]+ ms after/N ms after/' err.txt
