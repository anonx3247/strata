# Notes: proving the 17 goals

Final state: 17 of 17 goals proven (16 in memory.st, 1 in new.st). `./check.sh` at the last run shows 0 failed, 0 open in both files.

## Per goal

| Goal | File | Attempts | Result | Assumption |
|---|---|---|---|---|
| ids_none | memory.st | 1 | proven | none |
| ids_step | memory.st | 1 | proven | refinement: len |
| id_set_step | memory.st | 1 | proven | refinement: len |
| got_step | memory.st | 1 | proven | none |
| done_step | memory.st | 1 | proven | refinement: len |
| frame_ids | memory.st | 2 | proven | refinement: len |
| frame_got | memory.st | 2 | proven | none |
| outside_ids | memory.st | 2 | proven | refinement: len |
| outside_done | memory.st | 2 | proven | refinement: len |
| listed_push | memory.st | 1 | proven | none |
| some_new_here | memory.st | 1 | proven | refinement: len |
| some_new_kept | memory.st | 2 | proven | refinement: len |
| settled_insert | memory.st | 6 | proven | z3 (via settled_all) |
| unlocked_insert | memory.st | 2 | proven | z3 (via unlocked_all) |
| locked_insert | memory.st | 2 | proven | z3 (via locked_all) |
| unique_push | memory.st | 2 | proven | none |
| taken_ensures | new.st | 4 | proven | none |

Goals proven: 17 / 17. Goals resting on an assumption: 11 / 17 (8 on `refinement: len`, 3 on `z3`).

Helper lemmas added (not goals): `any_append`, `settled_one`, `settled_all`, `unlocked_one`, `unlocked_all`, `locked_one`, `locked_all`, `unique_append` in memory.st; `all_append`, `taken_fold` in new.st. Their proofs are checked in every run.

The z3 assumptions come from my own `nonlinear()` steps, used to show `k != id` from a fact `!(k == id || ...)`. I did not find a way to get that without z3 yet.

## Failures and what stopped me

- **Storing noise.** Every `./check.sh` prints "could not be stored ... doesn't check (N errors)" with "unknown variable `unfold`" and similar lines. The check still runs and the verdicts are correct. The message reads as a failure, so it is easy to misread.
- **frame_ids, attempt 1.** `assumption()` after `auto()`: "no goals left for `assumption` (the goal was closed by the tactics before it)". `auto()` had already closed the goal, so the extra step was the error. Same in frame_got, outside_ids, outside_done.
- **some_new_kept, attempt 1.** `assume()` in an `ensures added > 0 ==> ...` failed with "`assume` failed: the goal is neither `forall`, `==>` nor `!`". The `==>` in the ensures already gives `added > 0` as a fact, so `assume()` is not needed. The reference doesn't say this.
- **unique_append, attempt 1.** Goal `(unique(rest) && (!(y == x || C)...)) == (unique(rest) && (!(x == y || C)...))`: "nonlinear: the solver found no proof (sat)". The `==` in the two sides has its operands in opposite order, and the solver with a generic `T` does not see that they are equal. Specialising to `String` did not help. A split `if x == y` was needed.
- **unique_append, `assert (y == x) == (x == y) by { simplify() }`.** Error: "simp: no normal form within the rewrite budget (a rule that loops?): `x == y == (x == y)` applied 200 times". Equations between equations do not simplify the way I expected here.
- **settled_all, unlocked_all, locked_all.** Goal `k != task_of(y).id` with fact `!(k == task_of(y).id || contains(...))`: "`simp` failed: simp made no progress". `simplify` doesn't split a negated disjunction into a fact for the goal. Fixed by writing the goal as `k != ...` (the orientation of the fact) and then `nonlinear()`.
- **settled_all, first induction arm.** After `unfold(all, map, contains)`, `simplify(settled_all(rest))` failed with "`use` failed: `settled_all(...)` concludes `all(rest, ...)`, which doesn't match the goal". `simplify` had rewritten the lambda body in the goal, so the law's conclusion did not match it. Fixed by asserting `all(rest, ...)` first (from the law), then finishing with `simplify()`.
- **taken_fold.** Goal `all(fold([y] + rest, out, F), p)`: "`use` failed ... doesn't match the goal". The `fold` on a cons list was not simplified before the law call. Fixed by `simplify()` before it. After that, `simplify()` alone closed both branches, using the induction hypothesis.
- **Things the reference does not say:** that `auto` and `simplify` use an induction hypothesis (`ih`) on their own; that a fact's `!(A || B)` is not split for `simplify`; that an `ensures p ==> q` already gives `p`.

## Causes of failure, most common first

1. **Law conclusion does not match the goal after `simplify` rewrites inside a lambda.** `settled_all`: "`use` failed: `settled_all(...)` concludes `all(rest, ...)`, which doesn't match the goal". Fix: assert the `all(...)` fact first, then `simplify()`.
2. **Extra step after the goal is already closed.** frame_ids: "no goals left for `assumption` (the goal was closed by the tactics before it)". Also taken_fold: "no goals left for `use`". Fix: drop the step.
3. **Operand order in an equality with a generic type.** unique_append: "nonlinear: the solver found no proof (sat)" on `y == x` vs `x == y`. Also `simplify` looping on `(y == x) == (x == y)`. Fix: split on `if x == y` and state the lemma at `String`.
4. **A fact that is a negated disjunction is not used for a goal `!=`.** settled_all: "`simp` failed: simp made no progress" on `task_of(y).id != k`. Fix: write the goal as `k != ...` to match the fact, or use `nonlinear()` (which adds a z3 assumption).
5. **Ensures written as `p ==> q`.** some_new_kept: "`assume` failed: the goal is neither `forall`, `==>` nor `!`". The `p` is already a fact, so the step is not needed.
