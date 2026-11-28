#!/bin/bash
# usage: measure.sh SERVER CLIENT [N]: runs SERVER under /usr/bin/time -l,
# CLIENT against it, then stops SERVER; prints the client's line and the
# server's instructions, cycles and peak memory.
server=$1; client=$2
dir=$(mktemp -d); cd "$dir"
STRATA_MEMORY=${MEM:-} /usr/bin/time -l "$server" > server.out 2> time.out & s=$!

"$client" > client.out 2>&1
sleep 0.2
touch stop
wait $s
c=$(cat client.out)
i=$(grep "instructions retired" time.out | awk '{print $1}')
y=$(grep "cycles elapsed" time.out | awk '{print $1}')
m=$(grep "peak memory footprint" time.out | awk '{print $1}')
r=$(grep "maximum resident" time.out | awk '{print $1}')
echo "$c | instr $i | cycles $y | peak-footprint $m | max-rss $r"
cp time.out ${KEEP:-/dev/null} 2>/dev/null; grep -i "memory" time.out server.out | head -5
cd /; rm -rf "$dir"
