# NOTES

All 17 goals are proven in the final check. Attempts per goal (helpers are counted against the goal they were added for):
ids_none 2, ids_step 10 (budget spent; the final proof was only confirmed in runs through other goals' names, see below), id_set_step 5, got_step 1, done_step 0 (proven in a run for got_step, same proof as ids_step), frame_ids 7, frame_got 1, outside_ids 1, outside_done 1, listed_push 10 (proven on the 10th), some_new_here 1, some_new_kept 2, settled_insert 7, unlocked_insert 1, locked_insert 1, unique_push 2, taken_ensures 4 (3 at the proof, the first two with helpers).

Assumptions shown: `[assumed: refinement: len]` on take_gen, take_step, take_all, ids_step, id_set_step, done_step, frame_ids, outside_ids, outside_done, some_new_here, some_new_kept (anything that uses `take`/`len` of a list). No `[assumed: z3]` is left (contains_append first needed it).

ids_step: after 6 failed attempts the cause was in the helper `take_step`, not the goal. The goal itself went through only after the helpers were repaired through the id_set_step runs; its 10 attempts were already spent, so its last proof was seen only in other goals' runs.

## What stopped me (causes)
1. Conditions of a lemma call written as `a && b` are not met from separate facts; separate `requires` lines worked (take_gen). The call `take_step(rest, i - 1)` concludes `take(rest, i - 1 + 1)` and did not match the goal `take(rest, i)` ("which doesn't match the goal"). I added a `j == i + 1` parameter (take_gen).
2. `simplify` does not use a fact `x == t` (x a variable) to rewrite: "simp: no normal form within the rewrite budget (a rule that loops?)" or no progress. I used `assert (P(x) ==> Q) by { simplify_once(fact) assume() assumption() }` and `fact1()`.
3. Facts are not unfolded: a fact `listed(table, order)` or `dict_all(...)` stays folded and `auto()` can't instantiate it. Workaround: `assert F ==> unfolded by { simplify() }` and then `fact()`, and `fact1(j)` to instantiate a forall fact. A step to "unfold fact h" is missing.
4. Equality has no symmetry in simplify: fact `k != task_of(y).id` doesn't close `if task_of(y).id == k` ("unsolved goals", the goal printed `... if task_of(r).id == k {...}`). nonlinear() couldn't prove `k != x ==> x != k` for strings ("nonlinear: the solver found no proof (sat)"). Workaround: `if y == k { simplify() } else { auto() }`.
5. Boolean `||` and `&&` are not made associative/commutative: `(k == y || (A || B)) == (k == y || A || B)` stayed open. Needed helper lemmas or_comm/or_assoc/and_assoc. Passing `or_comm` to `simplify` loops; calling `or_comm()` as a step works.
6. A goal that is already `true` after a step (e.g. in `if a { ... }` branch) makes `simplify()` fail with "simp made no progress" with `⊢ true` printed; `auto()` closes it.
7. `take(xs, 0)` is not reduced without `match xs` (goal `take(tasks, 0)` stayed); `take(xs, len(xs)) == xs` needed its own lemma, and an `if len(rest) + 1 <= 0` split with `arithmetic()` on the impossible branch.
8. `assert i == i - 1 + 1 by { arithmetic() }` crashed the checker: `fault (Limit): stack overflow ... kernel_logic.normal_form (10231 times)`.
9. Stating `assume()` on a `p ==> q` statement fails ("the goal is neither forall, ==> nor !"): the statement was already read as requires/holds.
10. The attempt counter counts helper lemmas; failing helpers made the dependent goals fail with "`take_step` failed above: fix its proof first".
