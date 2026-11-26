#!/bin/zsh
# memcurve.sh OUT.csv CMD...: runs CMD and samples the resident memory of
# it and its children about every 50 ms (`ps -o rss=`, 20 ms apart). Writes `ms,rss_mb` lines
# to OUT.csv, then prints the peak, the time it was reached, the run's
# length and a 60-column text plot of the curve on stderr. CMD's own
# output goes where it would. Costs one `ps` and one `pgrep` a sample.
out=$1; shift
zmodload zsh/datetime
"$@" &
p=$!
t0=$EPOCHREALTIME
print "ms,rss_mb" > $out
while kill -0 $p 2>/dev/null; do
  pids=($p $(pgrep -P $p))
  for k in $(pgrep -P $p); do pids+=($(pgrep -P $k)); done
  kb=$(ps -o rss= -p ${(j:,:)pids} 2>/dev/null | awk '{s+=$1} END {print s+0}')
  ms=$(( (EPOCHREALTIME - t0) * 1000 ))
  printf "%d,%.1f\n" $ms $(( kb / 1024.0 )) >> $out
  sleep 0.02
done
wait $p
code=$?
awk -F, 'NR > 1 { t[n] = $1; r[n] = $2; if ($2 > peak) { peak = $2; at = $1 }; n++ }
  END {
    if (n == 0) exit
    printf "peak %.1f MB at %d ms of %d ms (%d samples)\n", peak, at, t[n-1], n > "/dev/stderr"
    cols = 60; rows = 8
    for (c = 0; c < cols; c++) { i = int(c * n / cols); h[c] = peak > 0 ? r[i] / peak * rows : 0 }
    for (y = rows; y >= 1; y--) {
      line = sprintf("%7.0f |", peak * y / rows)
      for (c = 0; c < cols; c++) line = line (h[c] >= y - 0.5 ? "#" : " ")
      print line > "/dev/stderr"
    }
    d = ""; for (c = 0; c < cols; c++) d = d "-"
    printf "        +%s\n         0 ms%49d ms\n", d, t[n-1] > "/dev/stderr"
  }' $out
exit $code
