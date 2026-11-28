# Notes

All 17 goals proven. 8 carry `[assumed: refinement: len]`: ids_step, id_set_step, done_step, frame_ids, outside_ids, outside_done, some_new_here, some_new_kept (they use len/slice). No z3 assumptions.

Attempts used (counted by check.sh; some re-runs of mine for reading output also counted):
- ids_none 1, ids_step 1, id_set_step 1, got_step 1, done_step 1, frame_ids 1, frame_got 1, outside_ids 1, outside_done 1: proven (unfold the helper fn, simplify with slice_step/map_append/set_push/slice_all).
- listed_push 4 (proven). Needed unfold(dict_all) as well as listed, then `assume(j)` and a `match table(j)` split.
- some_new_here 1, some_new_kept 1: proven with helper `any_snoc` (no `any` over append law is provided).
- settled_insert 9 (proven), unlocked_insert 1, locked_insert 1: each needs a helper by induction over the list (`settled_list`, `unlocked_list`, `locked_list`), then the goal is the helper applied to the slice `results[..i]`.
- unique_push 2 (proven) with helper `unique_snoc`; needed an `if y == x` split to make `y == x` and `x == y` meet.
- taken_ensures 3 (proven) with helpers `all_snoc` and `taken_acc` (fold generalised over its accumulator).

## What stopped me
- No lemma for `any`/`all`/`unique` over `xs + [x]` (any_snoc, all_snoc, unique_snoc); had to write each.
- A fact `!(A || B)` or `!contains(map([y]+rest, f), k)` is not used by simplify/auto to conclude `id != k`: `simplify made no progress`, and auto failed. Workaround: split `if id == k`, assert the positive fact (`contains(...)`), then `auto()` closes via the contradiction. simplify() alone did not close a goal with contradictory facts h6 / fact1.
- `simplify` after `unfold(settled_upto, ...)` expanded `settled` itself in the goal, so the goal no longer matched the helper; fix was to call the helper directly (the goal and the lemma match after simplification) instead of simplify(helper).
- `unfold(listed)` leaves `dict_all` folded in the facts; the quantifier is only instantiated after `unfold(listed, dict_all)`.
- Commutativity of `==` on strings between `y == x` and `x == y` inside `&&` was not normalised (`unique_snoc`): needed a case split on `y == x`.
- Error "no goals left for `cases`" when a simplify already closed a branch and I added a match: confusing but accurate.
- check.sh prints a block of "unknown variable `unfold` / `simplify`" errors at the top ("written as given: ... doesn't check"); they are harmless (the proofs still check) but noisy.
- Display of facts is not simplified (h6 shown as `!contains(map([y] + rest, ...))` even though simplify treats it as unfolded), which made it hard to see what simplify knew.
