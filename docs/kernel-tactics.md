# Writing proofs for `strata kernel check`

A proof script (`.kp` file) states theorems about Strata code and proves them with tactics, in Lean's shapes. `strata kernel goals FILE` prints each statement and its goal; `strata kernel check FILE` checks every proof and prints, for each theorem, `proven` (with the kernel's time), `failed` (the step and its goal), or `open` (no tactics yet).

## A script

```
-- a comment line
strata {
fn double(n: Int) -> Int = n + n
}

@[simp] theorem append_nil[A](xs: List[A]): xs + [] == xs := by
  induction xs with
  | nil => simp
  | cons x xs ih => simp [ih]

theorem double_pos(n: Int): n > 0 ==> double(n) > n := by
  intro h
  simp
  omega
```

- `strata { … }` holds Strata code: records, sums, functions (`fn`), `impl Ord for T { fn lt(self, other: T) -> Bool = … }`. Its functions become definitions with equations, and its `ensures` become goals named `f.ensures` (`f.ensures.0`, `.1`, … when there are several), proven with `theorem f.ensures := by …`.
- `theorem NAME[TYPES](PARAMS): STATEMENT := by` then the tactics, one or more per line, indented. The statement is a Strata `Bool` expression. The parameters are in scope as the goal's variables when the proof starts.
- `@[simp]` adds the theorem to the rules `simp` uses in the theorems after it.
- A theorem can use every theorem proven above it in the file, by name.

## Statements

Strata expressions over `Int`, `Bool`, `String` (no operations), `List[T]`, functions, and the script's records and sums:

- `+ - *` on `Int`; `+` on lists (append); `== != < <= > >=`; `&& || ! ==>`; `if c { a } else { b }`; `match` on a sum or list (`[]`, `[x, ..rest]`, constructors, `_`); `x := e` in a block.
- Calls `f(a, b)` and methods `xs.f(a)` (the same function: `xs.reversed()` is `reversed(xs)`); a record's field `u.age`; a record `User { name: n, age: a }` and an update `{ ..u, age: 3 }`.
- `forall(x -> p)` and `exists(x -> p)`; a lambda `x -> e` (`(x: Int) -> e` to give its type).
- Lists: `len(xs)`, `xs + ys`, `xs.reversed()`, `xs.map(f)`, `xs.sorted()` (on `List[Int]`), `[a, b]`, `[x] + xs`. Their definitions: `len([]) == 0`, `len([x] + xs) == 1 + len(xs)`; `[] + ys == ys`, `([x] + xs) + ys == [x] + (xs + ys)`; `[].reversed() == []`, `([x] + xs).reversed() == xs.reversed() + [x]`; `[].map(f) == []`, `([x] + xs).map(f) == [f(x)] + xs.map(f)`; `sorted` is insertion sort by `insert_sorted(x, xs)` (`x` before the first `y` with `x <= y`).

## Goals

A goal prints as Lean prints one: the variables and hypotheses, then `⊢` and what is left to prove.

```
  case cons
  x : A
  xs : List[A]
  ih : xs + [] == xs
  ⊢ [x] + xs + [] == [x] + xs
```

A list built by `cons` prints as `[x] + xs`, a list ending in `[]` as `[a, b]`.

## Tactics

Each tactic works on the first goal. A line can hold several, separated by `;`.

| Tactic | Does |
|---|---|
| `intro x y h` | Takes `forall(x -> …)` as the variable `x` and `p ==> q` as the hypothesis `h: p`, one name each, in order. `intro` alone takes one, naming it. |
| `induction xs with` | Structural induction on the variable `xs`: one alternative per constructor, `\| nil => tactics`, `\| cons x rest ih => tactics` (the constructor's arguments, then one hypothesis per recursive argument). A sum's constructors are its variants (`\| Red => …`, `\| SetTo v => …`); a record's is its name (`\| User n a => …`). Hypotheses that mention `xs` move into the goal as premises (`h ==> …`): take them back with `intro`. |
| `induction xs generalizing ys with` | The same, with `ys` quantified in the goal and in `ih` (`ih : forall(ys -> …)`): `intro ys` in each case. |
| `cases x with` | Induction without `ih`: one goal per constructor. |
| `cases h with` | On a hypothesis: `p && q` gives `\| intro hp hq`; `p \|\| q` gives `\| inl hp` and `\| inr hq`; `exists(x -> p)` gives `\| intro x hx`. |
| `rw [r1, ← r2]` | Rewrites the goal with each equation in turn, left to right (`←`: right to left): finds the first instance of its left side and replaces every copy of that instance. Closes the goal if it becomes `a == a`. `rw [h] at h2` rewrites hypothesis `h2`. |
| `simp` | Rewrites the goal to a normal form with the simp set: every definition's equations, the datatypes' facts (constructors are distinct and injective), `@[simp]` theorems, and logic (`true && p` is `p`, `x == x` is `true`, `if true { a } else { b }` is `a`, …). Closes the goal when it becomes `true`. Fails if nothing changed. |
| `simp [r1, r2]` | `simp` with these rules too (hypotheses, theorems, function names). A hypothesis `p` rewrites `p` to `true`; `!p` rewrites `p` to `false`. |
| `simp only [r1, r2]` | Only these rules (and beta reduction). |
| `simp at h` | Simplifies hypothesis `h`; closes the goal if `h` becomes `false`. |
| `exact t` | Closes the goal with `t`: a hypothesis, a theorem, or a theorem applied to terms and hypotheses: `exact append_nil(xs)`, `exact h(hx)` (premise `hx`). Arguments left out are found by matching the goal. |
| `apply t` | `t`'s conclusion (after its `forall`s and `==>` premises) matched against the goal; each premise becomes a goal. Give as arguments the variables the goal doesn't show: `apply le_trans(x, x + 1)`. |
| `calc` | A chain of equations, each with its proof: first line `calc a == b := by tactics`, then lines `_ == c := by tactics` indented under it (`_` is the previous right side). Ends at the goal's two sides. |
| `omega` | Linear integer arithmetic: closes a goal that follows from its hypotheses by `+ - *` (by constants), `< <= ==`, `&& \|\| ! ==>` on `Int` and `Bool`. Other subterms (`len(xs)`, `f(x)`) are unknowns: `len(xs) + 1 > len(xs)` is closed, `len(xs) >= 0` is not (it isn't arithmetic). |
| `rfl` | Closes `a == b` when the two sides are the same once lambdas are applied. |
| `constructor` | `p && q` as two goals `p`, `q`; `p == q` on `Bool` as `p ==> q` and `q ==> p`. |
| `left` / `right` | `p \|\| q`: prove `p` / prove `q`. |
| `by_cases h : p` | Two goals: one with `h : p`, then one with `h : !p`. |

Alternatives of `induction` and `cases` hold their tactics after `=>` on the same line, or on the lines below, indented further:

```
  induction xs with
  | nil => simp
  | cons x xs ih =>
    simp [ih]
    omega
```

After a tactic that leaves several goals (`constructor`, `by_cases`, `apply`), the next lines work on them in order; the goal must be closed when its tactics end.

## Errors

A failed step prints the tactic, why it failed and the goal it was given:

```
failed len_snoc (1.0 ms): `induction` failed: unsolved goals:
  case nil
  y : A
  ⊢ 1 + 0 == 0 + 1
```

- `unsolved goals`: the tactics ended with goals left (here: add `omega` after `simp`).
- `simp made no progress`: no rule applies; check the names and that the goal is unfolded.
- `its left side isn't in the goal` (`rw`): the equation's left side doesn't occur; `←` reverses it.
- `the goal doesn't follow by linear arithmetic` (`omega`): a fact is missing (rewrite with a lemma first) or the goal isn't linear.
- `no alternative | C … =>`: every constructor needs its alternative, by name.

## Hints

- Induction on the list a function matches on (`len`, `+`, `reversed` and `map` match on their first list).
- In the `cons` case, `simp [ih]` unfolds the definitions one step and uses the induction hypothesis; then `omega` for what is left in arithmetic.
- A lemma proven first (`theorem helper …`) and then used by `simp [helper]` or `rw [helper]` is the usual way through a harder goal.
- A theorem whose proof used `omega` is reported with `[oracle: omega]`: its arithmetic was decided by the solver, outside the kernel.
