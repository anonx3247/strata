# Writing proofs for `strata kernel check` (the law syntax)

This is the proof syntax planned for the rebuild (IDEAS.md, "Proofs for the kernel: laws, obligations and tactics"). A script (`.kp` file) states laws about Strata code and proves them. `strata kernel goals FILE` prints each statement and its goal. `strata kernel check FILE` checks every proof and prints, for each law, `proven` (with the kernel's time), `failed` (the step and its goal), or `open` (an empty proof). A law that rests on something outside the kernel's rules says so: `[assumed: refinement: len]`, `[assumed: z3]`. The same scripts can be written in Lean's syntax instead (docs/kernel-tactics.md); both can sit in one file.

## A script

```
-- a comment line
strata {
fn double(n: Int) -> Int = n + n
}

@[simp] law append_nil[T](xs: List[T]) holds xs + [] == xs by
  induction xs {
    [] => simp,
    [x, ..rest] => simp [append_nil(rest)],
  }

law double_more(n: Int) requires positive: n > 0 holds double(n) > n by
  simp
  arith

law double_zero holds double(0) == 0
```

- `strata { … }` holds Strata code: records, sums, functions, traits with laws, and `impl`s. Its functions become definitions with equations. Its `ensures` become goals named `f.ensures`, and each `impl` of a trait with laws gets one goal per law, `T.law`. Prove a goal with `law f.ensures by …`.
- `law NAME[TYPES](PARAMS) requires NAME: COND holds STATEMENT by TACTICS`. When the proof starts, the parameters are the goal's variables and each `requires` is a hypothesis under its name (`requires a: p requires b: q` for several). The statement is a Strata `Bool` expression.
- No `by` at all: the law is proven by the ladder, `auto` (`simp`, then `arith`, then Z3). `by` with nothing after it: the law is open.
- A parameter of a refined type (`n: Natural`) brings the hypothesis `n_ok : n >= 0`.
- `law NAME[T: Ord](…)`: a bound brings the variable `lt_T` (`T`'s `<`) and Ord's laws as hypotheses: `irreflexive : forall(a -> !(a < a))`, `transitive`, `total`. A law proven this way holds for every `Ord`; if it can't be proven from those, it fails, and it never becomes an obligation of the types.
- `@[simp]` adds the law to the rules `simp` uses in the laws after it.
- A law can use every law proven above it in the file, by name. A law that failed can't be used: the error says "`name` failed above".
- `mutual` … `end` around laws about types defined together (`Expr` and its `List[Expr]`) proves them at once. Each proof is one `induction` on the law's first parameter, and each law may call the others on smaller arguments.

## Statements

Strata expressions over `Int`, `Bool`, `String` (no operations), `List[T]`, functions, and the script's records and sums:

- `+ - *` on `Int`; `+` on lists (append); `== != < <= > >=`; `&& || ! ==>`; `if c { a } else { b }`; `match`; `x := e` in a block.
- Calls `f(a, b)` and methods `xs.f(a)` (the same function: `xs.reversed()` is `reversed(xs)`). A record's field is `u.age`. A record is `User { name: n, age: a }` and an update is `{ ..u, age: 3 }`.
- `forall(x -> p)` and `exists(x -> p)`; a lambda `x -> e`.
- Lists: `len(xs)` (a `Natural`), `xs + ys`, `xs.reversed()`, `xs.map(f)`, `xs.sorted()` (for any `Ord`), `[a, b]`, `[x] + xs`. Their definitions (core/models.strata):
  - `len([]) == 0` and `len([x] + xs) == 1 + len(xs)`
  - `[] + ys == ys` and `([x] + xs) + ys == [x] + (xs + ys)`
  - `[].reversed() == []` and `([x] + xs).reversed() == xs.reversed() + [x]`
  - `[].map(f) == []` and `([x] + xs).map(f) == [f(x)] + xs.map(f)`
  - `sorted` is insertion sort: `[].sorted() == []` and `([x] + xs).sorted() == insert_sorted(x, xs.sorted())`. `insert_sorted(x, [])` is `[x]`; `insert_sorted(x, [y] + ys)` is `[x, y] + ys` if `x <= y`, else `[y] + insert_sorted(x, ys)`.
- Types defined together: a list of a type inside it (`args: List[Expr]` in `Expr`) is a type of its own. It is still written `List[Expr]`, with `[]` and `[x, ..rest]`; `len`, `+` and `map` don't apply to it.

## Goals

A goal prints its variables and hypotheses, then `⊢` and what is left to prove:

```
  case cons
  x : T
  rest : List[T]
  ih : rest + [] == rest
  (`append_nil` may be called here on `rest`: it is `ih`)
  ⊢ [x] + rest + [] == [x] + rest
```

A `match` the goal can't unfold prints as `case_T(…)`; the error says which value it is stuck on, and `cases value { … }` splits it.

## Induction is recursion

`induction x { arms }` splits `x` by its constructors, one arm per constructor. In an arm, the law being proven can be called on a part of `x` the arm's pattern named (`append_assoc(rest, b, c)`): that call is the induction hypothesis. The other parameters can be anything, since the law holds for all of them. A call on anything else is an error. The goal lists the calls allowed.

## Tactics

One tactic per line, or several separated by `;`.

| Tactic | Does |
|---|---|
| `simp` | Rewrites to a normal form with the simp set: every definition's equations, the datatypes' facts (constructors are distinct and injective), `@[simp]` laws, logic, and linear arithmetic. Arithmetic means literals computed, sums collected (`a + 0` is `a`), and comparisons whose sides differ by a literal decided. An `if` left open is split, and the goal closes when both branches close. Fails if nothing changed. |
| `simp [l1, l2]` | `simp` with these rules too: hypotheses, laws, laws applied (`l(x)`), function names. A hypothesis `p` rewrites `p` to `true`; `!p` rewrites `p` to `false`; `x == y` rewrites `x` to `y`. |
| `simp once [l]` | One rewrite with `l`: the first instance of its left side, every copy of it. `simp once [← l]` goes right to left. Use it when the normal form goes the wrong way or loops. |
| `simp at h`, `simp [l] at h1 h2` | Simplifies hypotheses. A hypothesis that becomes `false` closes the goal. |
| `unfold f g` | Unfolds the definitions of `f` and `g` only. |
| `compute` | `simp`. |
| `use l(args)` | Closes the goal with a hypothesis or a law applied to terms and hypotheses (`use lt_trans(a, b, c)(h1)(h2)`). If the law has premises left, they become goals. |
| `lemma h: p by tactics` | Proves `p` (the tactics after `by`, or on the lines under it), then adds the hypothesis `h : p` for the rest of the proof. |
| `cases e { arms }` | One goal per constructor of `e`'s type. The arms are `[] => …`, `[x, ..rest] => …`, `Red => …`, `Some(v) => …`, `User { name, age } => …`. When `e` isn't a variable, each arm has the hypothesis `h : e == pattern` (`cases e as eq { … }` names it). On a hypothesis: `p && q` is `intro(hp, hq) => …`, `p \|\| q` is `inl(hp) => …, inr(hq) => …`, and `exists(x -> p)` is `intro(x, hx) => …`. |
| `induction x { arms }` | As `cases`, with the law callable on the parts. |
| `calc` | A chain of equations, each with its proof, on the lines under it: `a == b by tactics`, then `_ == c by tactics` (`_` is the previous right side). It ends at the goal's two sides. |
| `witness e` | On `exists(x -> p)`: prove `p` at `e`. |
| `arith` | Linear integer arithmetic, by kernel steps. It closes a goal that follows from its hypotheses by `+ - *` (by literals), `< <= ==`, and `&& \|\| ! ==>` on `Int` and `Bool`. Other subterms are unknowns. Refined results count as facts: `len(xs) >= 0` holds, since `len` returns a `Natural`. |
| `nonlinear` | Asks Z3 (strings, products of unknowns). The law is marked `[assumed: z3]`. |
| `auto` | The ladder: `simp`, then `arith`, then `nonlinear`. |
| `constructor` | Splits `p && q` into two goals, and `p == q` on `Bool` into `p ==> q` and `q ==> p`. |
| `left` / `right` | On `p \|\| q`: prove `p`, or prove `q`. |
| `intro x h` | Takes a `forall`'s variable or a `==>`'s premise. |
| `exfalso`, `contradiction` | Changes the goal to `false`; closes it from `false`, or from `p` and `!p`. |
| `by_cases h : p` | Two goals: one with `h : p`, then one with `h : !p`. |
| `split` | On the first open `if` of the goal: two goals, one with its condition and one with its negation. |
| `try t`, `t1 <;> t2` | `t`, or nothing if it fails; `t2` on every goal `t1` leaves. |

An arm's tactics come after `=>`, separated by `;` or on the arm's own line. For several lines, use braces:

```
  induction xs {
    [] => simp,
    [x, ..rest] => {
      simp [len_append(rest, ys)]
      arith
    },
  }
```

After a tactic that leaves several goals (`constructor`, `by_cases`, `split`, `use`), the next lines work on them in order. A tactic after the last goal is closed is an error ("no goals left"): use `try` for steps that may already be done.

## Errors

- `unsolved goals`: the tactics ended with goals left.
- `no goals left for …`: a tactic after the goal was closed.
- `simp made no progress`: no rule applies. A stuck `match` is named.
- `arith: no contradiction found`: a fact is missing (a lemma, `cases` on a stuck `match`), or the goal isn't linear (`nonlinear`).
- `no alternative for C`: every constructor needs its arm.
- "`l` is being proven: it can be called only in an induction's branch": a recursive call on something that isn't a part of the value split.

## Hints

- Do induction on the list a function matches on: `len`, `+`, `reversed` and `map` match on their first list, `insert_sorted` on its list.
- In the `[x, ..rest]` arm, `simp [law(rest, …)]` unfolds the definitions one step and uses the recursive call; arithmetic left over closes by `simp` or `arith`.
- Prove a helper law first, then use it with `simp [helper]` or `use helper(…)`.
- For `Ord` laws on a sum, `cases` every variable and `simp`.
