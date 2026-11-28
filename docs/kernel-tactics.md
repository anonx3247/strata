# Writing proofs for `strata kernel check` (Lean's syntax)

A proof script (`.kp` file) states theorems about Strata code and proves them with tactics, in Lean's shapes. `strata kernel goals FILE` prints each statement and its goal. `strata kernel check FILE` checks every proof and prints, for each theorem, `proven` (with the kernel's time), `failed` (the step and its goal), or `open` (no tactics yet). A theorem that rests on something outside the kernel's rules says so: `[assumed: refinement: len]`, `[assumed: z3]`. The same goals written as Strata laws, in modules, are in tests/kernel/laws (docs/kernel-laws.md); this syntax is the baseline to compare against.

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

- `strata { … }` holds Strata code: records, sums, functions, traits with laws, and `impl`s. Its functions become definitions with equations. Its `ensures` become goals named `f.ensures` (`f.ensures.0`, `.1`, … when there are several). Each `impl` of a trait with laws gets one goal per law, `T.law` (`Priority.transitive`). Prove a goal with `theorem f.ensures := by …`.
- `theorem NAME[TYPES](PARAMS): STATEMENT := by` then the tactics, one or more per line, indented. The statement is a Strata `Bool` expression. When the proof starts, the parameters are the goal's variables. Each parameter of a refined type (`n: Natural`) also brings the hypothesis `n_ok : n >= 0`.
- `theorem NAME[A: Ord](xs: List[A]): …`: a bound brings the variable `lt_A` (`A`'s `<`, printed `a < b`) and Ord's laws as hypotheses `irreflexive : forall(a -> !(a < a))`, `transitive`, `total`, and the derived fact `asymmetric : forall(a -> forall(b -> a < b ==> !(b < a)))` (proven once, in core/ord.kp, and used by `simp`). A theorem proven this way holds for every `Ord`. On an `Ord`, `a <= b` is `!(b < a)`; on `Int`, `simp` writes `!(a < b)` as `b <= a`.
- `@[simp]` adds the theorem to the rules `simp` uses in the theorems after it.
- A theorem can use every theorem proven above it in the file, by name. A theorem that failed can't be used: the error says "`name` failed above".
- `mutual` … `end` around theorems about types defined together (`Expr` and its `List[Expr]`) proves them by one induction. Each proof is one `induction` on its first parameter.

## Statements

Strata expressions over `Int`, `Bool`, `String` (no operations), `List[T]`, functions, and the script's records and sums:

- `+ - *` on `Int`; `+` on lists (append); `== != < <= > >=`; `&& || ! ==>`; `if c { a } else { b }`; `match` on a sum or list (`[]`, `[x, ..rest]`, constructors, `_`); `x := e` in a block.
- Calls `f(a, b)` and methods `xs.f(a)` (the same function: `xs.reversed()` is `reversed(xs)`). A record's field is `u.age`. A record is `User { name: n, age: a }` and an update is `{ ..u, age: 3 }`.
- `forall(x -> p)` and `exists(x -> p)`; a lambda `x -> e` (`(x: Int) -> e` to give its type).
- Lists: `len(xs)` (a `Natural`), `xs + ys`, `xs.reversed()`, `xs.map(f)`, `xs.sorted()` (for any `Ord`), `[a, b]`, `[x] + xs`. Their definitions (core/models.strata):
  - `len([]) == 0` and `len([x] + xs) == 1 + len(xs)`
  - `[] + ys == ys` and `([x] + xs) + ys == [x] + (xs + ys)`
  - `[].reversed() == []` and `([x] + xs).reversed() == xs.reversed() + [x]`
  - `[].map(f) == []` and `([x] + xs).map(f) == [f(x)] + xs.map(f)`
  - `sorted` is insertion sort: `[].sorted() == []` and `([x] + xs).sorted() == insert_sorted(x, xs.sorted())`. `insert_sorted(x, [])` is `[x]`; `insert_sorted(x, [y] + ys)` is `[x, y] + ys` if `x <= y`, else `[y] + insert_sorted(x, ys)`.
- Types defined together: a list of a type inside it (`args: List[Expr]` in `Expr`) is a type of its own (`List_Expr`). It is still written `List[Expr]`, with `[]` and `[x, ..rest]`; `len`, `+` and `map` don't apply to it.

## Goals

A goal prints as Lean prints one: the variables and hypotheses, then `⊢` and what is left to prove.

```
  case cons
  x : A
  xs : List[A]
  ih : xs + [] == xs
  ⊢ [x] + xs + [] == [x] + xs
```

A list built by `cons` prints as `[x] + xs`, a list ending in `[]` as `[a, b]`, and a record by its fields (`User { name: u.name, age: u.age }`). A bounded function prints without its dictionary (`insert_sorted(x, xs)`, written back the same way). A `match` the goal can't unfold prints as `match value { [] => …, [y, ..more] => … }`; the error names the value, and `cases h : value with` splits it. Every record has the law `T { ..t } == t` built in, which `simp` uses (`User { name: u.name, age: u.age }` is `u`).

## Tactics

Each tactic works on the first goal. A line can hold several, separated by `;`. Lines indented under a tactic that doesn't take lines (anything but `induction`, `cases … with`, `calc`, `have … := by`) continue the proof after it.

| Tactic | Does |
|---|---|
| `intro x y h` | Takes `forall(x -> …)` as the variable `x`, and `p ==> q` (or `!p`, as `p ==> false`) as the hypothesis `h: p`, one name each, in order. `intro` alone takes one, naming it. |
| `induction xs with` | Structural induction on the variable `xs`, with one alternative per constructor: `\| nil => tactics` and `\| cons x rest ih => tactics` (the constructor's arguments, then one hypothesis per recursive argument). A sum's constructors are its variants (`\| Red => …`, `\| SetTo v => …`); a record's is its name (`\| User n a => …`). Hypotheses that mention `xs` come back in each case under their names, holding for the new value. |
| `induction xs generalizing ys with` | The same, with `ys` quantified in `ih` (`ih : forall(ys -> …)`). `ys` is back in scope in each case. The theorem's other parameters are always generalized, in their order: for `theorem t(x, xs, y)` with induction on `xs`, `ih : forall(x -> forall(y -> premises ==> …))`, applied as `ih(x, y)(h)`. |
| `cases x with` | Induction without `ih`: one goal per constructor. |
| `cases h : t with` | Cases on any term `t`, with the hypothesis `h : t == C(…)` in each case. Use it when a `match` is stuck on `t`. |
| `cases h with` | On a hypothesis: `p && q` gives `\| intro hp hq`; `p \|\| q` gives `\| inl hp` and `\| inr hq`; `exists(x -> p)` gives `\| intro x hx`. |
| `rw [r1, ← r2]` | Rewrites the goal with each equation in turn, left to right (`←`: right to left). It finds the first instance of the left side and replaces every copy of that instance, then closes the goal if it becomes `a == a`. `rw [h] at h1 h2` rewrites hypotheses. A hypothesis `h : x == y` rewrites `x` to `y`. |
| `simp` | Rewrites to a normal form with the simp set: every definition's equations, the datatypes' facts (constructors are distinct and injective), `@[simp]` theorems, logic (`true && p` is `p`, `x == x` is `true`, `if true { a } else { b }` is `a`), and linear arithmetic. Arithmetic means literals computed (`1 + 2` is `3`), sums collected (`a + 0` is `a`, `0 + a + 1` is `a + 1`), and `a < b`, `a <= b`, `a == b` decided when the sides differ by a literal. If an `if` stays open, `simp` tries both branches and closes the goal when both close. It closes the goal when it becomes `true`, and fails if nothing changed. |
| `simp [r1, r2]` | `simp` with these rules too: hypotheses, theorems, function names. A hypothesis `p` rewrites `p` to `true`; `!p` rewrites `p` to `false`. |
| `simp only [r1, r2]` | Only these rules, logic, literals and lambdas applied. |
| `simp at h`, `simp [r] at h1 h2` | Simplifies hypotheses. A hypothesis that becomes `false` closes the goal. |
| `unfold f g` | `simp only [f, g]`. |
| `exact t` | Closes the goal with `t`: a hypothesis, or a theorem or hypothesis applied to terms and facts (`exact append_nil(xs)`, `exact h(hx)`, `exact ih(x, z)(h5)`, `exact lt_trans(a, b, c)(h1)(h2)`; a premise `p && q` takes its two facts one at a time). Arguments left out are found by matching the goal. A fact and a premise, or the theorem and the goal, that differ until simplified (a `match` unfolded) still match. |
| `apply t` | Matches `t`'s conclusion (after its `forall`s and `==>` premises) against the goal; each premise becomes a goal. Give as arguments the variables the goal doesn't show: `apply le_trans(x, x + 1)`. On the goal `false`, `apply h` with `h : !p` leaves the goal `p`. |
| `use t` | `exact t`, else `apply t`. |
| `have h : p := by tactics` | Proves `p`, then adds the hypothesis `h : p`. `have h := lemma(args)` adds a theorem's instance. |
| `calc` | A chain of equations, each with its proof. The first line is `calc a == b := by tactics`; then come lines `_ == c := by tactics`, indented under it (`_` is the previous right side). The chain ends at the goal's two sides. |
| `omega` | Linear integer arithmetic, by kernel steps. It first simplifies its hypotheses and the goal (`Task { priority: Low, due: a } < Task { priority: Low, due: b }` becomes `a < b`), then closes a goal that follows by `+ - *` (by literals), `< <= ==`, and `&& \|\| ! ==>` on `Int` and `Bool`. Other subterms are unknowns: `len(xs) + 1 > len(xs)` holds. Refined results count as facts: `len(xs) >= 0` holds, since `len` returns a `Natural`. |
| `nonlinear` | Asks Z3 (strings, products of unknowns). The theorem is marked `[assumed: z3]`. |
| `auto` | Tries `simp` with every hypothesis, then `omega`, then `nonlinear`; first it applies the laws among the hypotheses (`transitive`) to the facts once. |
| `assumption` | Closes the goal with a hypothesis that is it. |
| `rfl` | Closes `a == b` when the two sides are the same once lambdas are applied. |
| `constructor` | Splits `p && q` into two goals `p` and `q`, and `p == q` on `Bool` into `p ==> q` and `q ==> p`. |
| `left` / `right` | On `p \|\| q`: prove `p`, or prove `q`. |
| `exists e` | On `exists(x -> p)`: prove `p` at `e`. |
| `exfalso` | Changes the goal to `false`. |
| `contradiction` | Closes the goal from a hypothesis `false`, or from `p` and `!p`. |
| `by_cases h : p` | Two goals: one with `h : p`, then one with `h : !p`. |
| `split`, `split h` | On the first open `if c { … }` of the goal: two goals, first the `then` case with `h : c` and its `if` taken, then the `else` case with `h : !c` (`!c` written without a double negation). `split h` names the hypothesis; plain `split` picks a fresh name (`h`, `h1`, …). |
| `try t` | Runs `t`; if it fails, or no goal is left, nothing changes. |
| `· tactics` | The next goal alone, closed by these tactics (and the lines indented under them). |
| `t1 <;> t2` | Runs `t2` on every goal `t1` leaves. |

Alternatives of `induction` and `cases` hold their tactics after `=>` on the same line, or on the lines below, indented further:

```
  induction xs with
  | nil => simp
  | cons x xs ih =>
    simp [ih]
    omega
```

After a tactic that leaves several goals (`constructor`, `by_cases`, `apply`, `split`), the next lines work on them in order; the goal must be closed when its tactics end. A tactic after the last goal is closed is an error ("no goals left"): use `try` for steps that may already be done.

## Recursion: induction by calling the theorem

In an induction's case, the theorem being proven can be called on a recursive argument of the case: `exact append_assoc(rest, ys, zs)`, `simp [len_append(rest, ys)]`. That call is the induction hypothesis. The goal lists the calls allowed: "(`append_assoc` may be called here on `rest`: it is `ih`)". A call on anything else is an error. In a `mutual` block, the block's other theorems may be called the same way, on arguments of their types.

## Errors

A failed step prints the tactic, why it failed, and the goal it was given:

```
failed len_snoc (1.0 ms): `induction` failed: unsolved goals:
  case nil
  y : A
  ⊢ len([y]) == 1
```

- `unsolved goals`: the tactics ended with goals left.
- `no goals left for …`: a tactic after the goal was closed. Remove it, or wrap it in `try`.
- `rw` matches terms as written: `[a] + [b] + rest` and `[a] + ([b] + rest)` are different terms to it (`simp` normalises lists, `rw` doesn't).
- `simp made no progress`: no rule applies. Check the names and that the goal is unfolded. A stuck `match` is named.
- `its left side isn't in the goal` (`rw`): the equation's left side doesn't occur. `←` reverses it.
- `omega: no contradiction found`: a fact is missing (rewrite with a lemma, `cases` a `match`), or the goal isn't linear (`nonlinear`).
- `no alternative for C`: every constructor needs its alternative, by name.

## Hints

- Do induction on the list a function matches on: `len`, `+`, `reversed` and `map` match on their first list, `insert_sorted` and `count` on their list.
- In the `cons` case, `simp [ih]` unfolds the definitions one step and uses the induction hypothesis; arithmetic left over usually closes by itself, else with `omega`.
- Prove a lemma first (`theorem helper …`), then use it with `simp [helper]`, `rw [helper]` or `exact helper(…)`. That is the usual way through a harder goal. Any law or hypothesis can be used inside another step: `have h := transitive(a, b, c)(h1)(h2)`.
- For `Ord` laws on a sum, `cases` every variable and `simp`: `Low < Mid` unfolds through `impl Ord` to literals, which `simp` computes.
