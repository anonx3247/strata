#!/bin/zsh
# memprof.sh LABEL STRATA HOME [BUILD FLAGS...]: the memory profile of the
# workloads in tools/memprof (and `strata check` of compiler/ with STRATA
# itself), into target/memprof/LABEL: each one's memory curve (`NAME.csv`,
# from tools/memcurve.c: resident size and footprint every 10 ms), its run's output and the runtime's report
# (`NAME.out`: STRATA_MEMORY's tallies, and with `--memory-sites` the
# sites and, with STRATA_MEMORY_LIVE, the live objects at the peak).
# Programs are built `--release --no-verify` with the flags given. With
# `--memory-sites` (and a STRATA built with it, for `check`), every site
# goes to `NAME.sites.tsv` and the live objects at the peak by site to
# `NAME.live.tsv` (STRATA_MEMORY_SITES, STRATA_MEMORY_LIVE). Run one at a
# time: each is a heavy command.
label=$1; strata=$2; home=$3; shift 3
root=${0:A:h:h}
out=$root/target/memprof/$label
mkdir -p $out
clang -O2 -o $root/target/memcurve $root/tools/memcurve.c || exit 1
for p in json serve pipeline spawns; do
  rm -f $out/$p.strata
  $strata --file $out/$p.strata edit < $root/tools/memprof/$p.st > /dev/null 2>&1
  STRATA_HOME=$home $strata build $out/$p.strata --no-verify --release "$@" -o $out/$p > $out/$p.build 2>&1 || { echo "$p: build failed (see $out/$p.build)"; continue; }
  STRATA_MEMORY=1 STRATA_MEMORY_SITES=$out/$p.sites.tsv STRATA_MEMORY_LIVE=$out/$p.live.tsv \
    $root/target/memcurve $out/$p.csv $out/$p > $out/$p.out 2>&1
  echo "$p: $(grep '^peak' $out/$p.out)"
done
(cd $root/compiler && STRATA_MEMORY=1 STRATA_MEMORY_SITES=$out/check.sites.tsv STRATA_MEMORY_LIVE=$out/check.live.tsv STRATA_HOME=$home $root/target/memcurve $out/check.csv $strata check > $out/check.out 2>&1)
echo "check: $(grep '^peak' $out/check.out)"
