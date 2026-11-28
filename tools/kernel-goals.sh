#!/bin/bash
# tools/kernel-goals.sh OUT_DIR — writes memory.st and new.st: the real definitions (from
# examples/tasks, core and compiler/new, as `strata view` prints them) and
# each of the 17 obligations #677 no longer proves as an open lemma.
set -e
W=$(cd "$(dirname "$0")/.." && pwd)
out=$1; mkdir -p $out; tmp=$(mktemp -d)
export STRATA_HOME=$W
st=${STRATA:-$W/target/dev/strata}
(cd $W/examples/tasks && $st view model --proofs) > $tmp/model.st
(cd $W/examples/tasks && $st view memory --proofs) > $tmp/memory.st
$st --file $W/compiler view new --proofs > $tmp/new.st
$st --file $W/core/types.strata view main > $tmp/types.st
$st --file $W/std view time > $tmp/time.st
# the paragraph defining NAME (definitions are separated by blank lines)
def() { awk -v n="$2" 'BEGIN{RS=""; ORS="\n\n"} { if (match($0, "(^|\n)(pub )?(distinct )?(fn|lemma|type) " n "[[(= ]")) print }' "$1"; }
# a lemma with its body replaced by an open proof
goal() { def "$1" "$2" | perl -0pe 's/\n+\z//; if (/ \{\n/) { s/ \{\n.*\z/ {\n  ?proof\n}/s } else { $_ .= " {\n  ?proof\n}" } $_ .= "\n\n"'; }
{
  echo "// Goals: the obligations of examples/tasks' memory module that #677 no longer proves"
  echo "// (per-instance quantifier facts). Definitions as in examples/tasks/model.strata,"
  echo "// examples/tasks/memory.strata and std/time.strata."
  echo
  def $tmp/time.st Instant
  for n in TaskId WorkerId Task TaskAttempt TaskResult Status Entry task_of ids after_attempt locked all_locked settled unlocked; do def $tmp/model.st $n; done
  for n in ids_upto some_new listed got_ids done_ids settled_upto id_set unlocked_upto; do def $tmp/memory.st $n; done
  for n in ids_none ids_step id_set_step got_step done_step frame_ids frame_got outside_ids outside_done listed_push some_new_here some_new_kept settled_insert unlocked_insert locked_insert unique_push; do goal $tmp/memory.st $n; done
} > $out/memory.st
{
  echo "// Goal: compiler/new.strata's \`taken\` meets its \`ensures\`, which #677 no longer proves"
  echo "// (its loop invariant: an \`all\` whose element fact is an \`any\`)."
  echo
  def $tmp/new.st taken
  printf '/// `taken` meets its `ensures`.\nlemma taken_ensures() {\n  ?proof\n}\n'
} > $out/new.st
