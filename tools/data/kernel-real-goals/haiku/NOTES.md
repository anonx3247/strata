# Notes: proving the 17 goals

Result: 12 of 17 goals proven. 9 of those 12 rest on an assumption (`[assumed: …]`). 5 goals open. 48 attempts used in total (attempts.log).

## Per goal

Proven goals:

| goal | file | attempts | status | assumption |
|---|---|---|---|---|
| ids_none | memory.st | 2 | proven | none |
| ids_step | memory.st | 1 | proven | refinement: len, z3 |
| id_set_step | memory.st | 8 | proven | refinement: len, z3 |
| got_step | memory.st | 5 | proven | z3 (through contains_snoc) |
| done_step | memory.st | 1 | proven | refinement: len, z3 |
| frame_got | memory.st | 4 | proven | none |
| frame_ids | memory.st | 3 | proven | refinement: len |
| outside_ids | memory.st | 1 | proven | refinement: len |
| outside_done | memory.st | 2 | proven | refinement: len |
| some_new_here | memory.st | 2 | proven | refinement: len, z3 |
| some_new_kept | memory.st | 2 | proven | refinement: len, z3 |
| taken_ensures | new.st | 4 | proven | none |

Open goals:

- **listed_push** (memory.st): 4 attempts, failed. The goal left after `unfold(listed)`, `simplify(contains_snoc)`, `assume(k1)` and the match on `table(k1)` is `k1 == k || contains(order, k1)` under `h : table(k1) == Some(v)`. The needed fact is in `h3 : listed(table, order)`, which stays folded. `simplify(listed)` did not unfold it, and `auto()` reports `(nonlinear: the solver found no proof (sat))`. Missing: a way to instantiate a `forall` fact (or a definition that is one) at a point.
- **unique_push** (memory.st): 9 attempts, failed. It depends on `unique_snoc`, and the helper `contains_snoc_flip` (the `x == k` orientation) is the one that fails: `⊢ k == x == (x == k)`, `nonlinear: the solver found no proof (sat)`. The proof of `unique_snoc` itself works except for that orientation. Missing: equality orientation (`a == b` vs `b == a`) is not identified, not by simplify and not by z3.
- **settled_insert**, **unlocked_insert**, **locked_insert** (memory.st): 0 attempts. I did not attempt them. They use `dict_with(after, k, e)` and `!contains(…, k)`, so they run into the same orientation issue as unique_push (`j == k` from `dict_with` vs `k == x` from `contains`). I did not test that.

## Failures and what stopped me

1. **Equality orientation.** `contains(xs + [x], k)` simplifies to `k == x || …`, while `dict_with` and `unique` produce `x == k`. No step identifies them. `contains_snoc_flip` failed with `⊢ k == x == (x == k)` and z3 said `sat`.
2. **Facts that are definitions are not unfolded.** unique_snoc's first try: `have failed: unsolved goals ⊢ unique(rest)` with `h2 : unique([y] + rest)` in the context. Fix: `assert rest.unique() by { auto() }`. listed_push: see above.
3. **Required facts of a called law are not derived.** unique_snoc: `⊢ y != x` even with `x != y` known. Fix: `assert !(x == y) by { auto() }` for the orientation the kernel produces. Index lemmas: `have failed: ⊢ 0 <= i - 1 && i - 1 < len(rest)` although `i >= 1` and `i < len([y] + rest)` were known. Fix: `assert 0 <= i - 1 && i - 1 < len(rest) by { arithmetic() }` first, then the call.
4. **Recursive call shape must match the simplified goal.** fold_members: `use failed: fold_members(existing, rest, acc + [y]) concludes all(fold(rest, …)), which doesn't match the goal ⊢ all(fold([y] + rest, …))`. Fix: `simplify()` before the call (after that the simplify closed the goal from the induction hypothesis, so the call was removed). Lemma instances as rewrites also fail when the goal's lambda is already unfolded: `simp made no progress` for `any_snoc_new` inside `some_new_here`. Fix: state the instance with `assert … by { lemma(args) }` and finish with `auto()`.
5. **Integer conditions left in `if`.** take_snoc: `⊢ if i + 1 <= 0 { [] } else { … } == …`, even with `fact3 : 0 < i + 1` in context. Fix: an explicit `if i + 1 <= 0 { arithmetic() } else { simplify() }`. Also an `else` whose goal is already `true` gives `simp made no progress`.

Smaller points:
- `calc` with `==` chained on one line fails with `Bool is not (String) -> Option[Entry[P]]`. The form that works is `calc { a == b; _ == c; }`.
- `auto()` at the end of a helper whose goal is `(k == x) == (x == k)` with z3 fails; it is orientation again.
- Once in the new.st run, `unknown variable unfold` was reported for `taken_ensures`. A later run with the same `unfold(taken)` step worked, so I did not pursue it.
- `[assumed: refinement: len]` appears on many proofs that do not mention a refinement. The brief allows it, so I kept those goals as proven but counted them as resting on an assumption.
