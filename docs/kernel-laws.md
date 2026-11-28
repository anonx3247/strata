# Writing laws and proofs in Strata (`strata kernel check MODULE.strata`)

A law and its proof are Strata code: a definition in a module, stored in the tree like any other. `strata kernel check FILE.strata` checks every law of the module and prints, for each, `proven` (with the kernel's time), `failed` (the step and its goal), or `open`. A law that rests on something outside the kernel's rules says so: `[assumed: refinement: len]`, `[assumed: z3]`. `strata kernel goals FILE.strata` prints each law's goal.

In the lab a law is written `lemma` (the rebuild's `law`), its statement is its `ensures` (the rebuild's `holds`), and its proof is its body. The proof steps aren't functions `strata check` knows, so store such a module with `strata --file F.strata edit --as-given < text`. The same goals in Lean's syntax are in docs/kernel-tactics.md.

## A module

```
fn double(n: Int) -> Int = n + n

/// `xs + []` is `xs`.
lemma append_nil[T](xs: List[T]) ensures xs + [] == xs {
  match xs {
    [] => simplify(),
    [_, ..rest] => simplify(append_nil(rest)),
  }
}

lemma double_more(n: Int) requires n > 0 ensures double(n) > n {
  unfold(double)
  arithmetic()
}

lemma len_nonneg[T](xs: List[T]) ensures len(xs) >= 0

lemma open_one(xs: List[Int]) ensures len(xs.sorted()) == len(xs) {
  ?proof
}
```

- The module's records, sums, functions, traits with laws and `impl`s are the code the laws are about. Each function becomes a definition with equations.
- `lemma NAME[TYPES](PARAMS) requires COND ensures STATEMENT { PROOF }`. The parameters are the law's variables, and each `requires` is an assumption. Nothing is introduced by hand: what a `for all` or `==>` would say is a parameter or a `requires`. A law whose statement is `!p` assumes `p` and proves `false`.
- No body: the law is proven by the ladder, `auto()`. A body `?proof`: the law is open.
- A function's `ensures` is the goal `f.ensures`, and an `impl`'s trait laws are goals `T.law`. A `lemma` with no `ensures` proves the goal its name spells: `twice_ensures` proves `twice.ensures`, and `impl_priority_irreflexive` proves `Priority.irreflexive` (an `impl` obligation takes the prefix `impl_`).
- A parameter of a refined type (`n: Natural`) brings the assumption `n >= 0`.
- `[A: Ord]` brings `A`'s `<` and Ord's laws as assumptions: `irreflexive : forall(a -> !(a < a))`, `transitive`, `total`. A law proven so holds for every `Ord`; if it can't be proven from those, it fails, and it never becomes an obligation.
- A law can use any other law of the module, by calling it. Laws that call each other in their proofs are proven together, by one induction, so each must be a `match` on its first parameter.

## Statements

Strata expressions over `Int`, `Bool`, `String` (no operations), `List[T]`, functions, and the module's records and sums:

- `+ - *` on `Int`; `+` on lists (append); `== != < <= > >=`; `&& || ! ==>`; `if`; `match` (list patterns of any length, constructors, `_`); `x := e`.
- Calls `f(a, b)` and methods `xs.f(a)` (the same function). A record's field is `u.age`. A record is `User { name: n, age: a }` and an update is `{ ..u, age: 3 }`. `forall(x -> p)` and `exists(x -> p)`; a lambda `x -> e`.
- A list literal can't spread another list: write `[x] + rest`, not `[x, ..rest]` (that is a pattern only).
- Lists: `len(xs)` (a `Natural`), `xs + ys`, `xs.reversed()`, `xs.map(f)`, `xs.sorted()` (for any `Ord`). Their definitions are in core/models.strata:
  - `len([]) == 0` and `len([x] + xs) == 1 + len(xs)`
  - `[] + ys == ys` and `([x] + xs) + ys == [x] + (xs + ys)`
  - `[].reversed() == []` and `([x] + xs).reversed() == xs.reversed() + [x]`
  - `[].map(f) == []` and `([x] + xs).map(f) == [f(x)] + xs.map(f)`
  - `sorted` is insertion sort: `([x] + xs).sorted() == insert_sorted(x, xs.sorted())`. `insert_sorted(x, [y] + ys)` is `[x, y] + ys` if `x <= y`, else `[y] + insert_sorted(x, ys)`.
- A list of a type inside the type (`args: List[Expr]` in `Expr`) is a type of its own. It is still written `List[Expr]`, but `len`, `+` and `map` don't apply to it.

## Proof steps

A proof is a block of steps, one per line. Each step works on what is left to prove (the goal), and a branch's steps must finish it.

| Step | Does |
|---|---|
| `match x { arms }` | Splits on `x`, one arm per constructor (`[] => …`, `[y, ..rest] => …`, `Leaf(n) => …`, `User { name, age } => …`, `_ => …`). It is induction: in an arm, the law can call itself on a part of `x` its pattern named (`append_assoc(rest, b, c)`), and the call is checked to be on a smaller value. Splitting a part again (`match rest`) keeps the call valid on the new parts (`[z] + more`). On a value that isn't a variable, each arm also knows `value == pattern`. |
| `if c { … } else { … }` | Splits on a condition. In each branch, `c` (or `!c`) is a known fact, and the goal's `if c` are taken. |
| `simplify(l1, l2)` | Rewrites the goal to a normal form. It uses the laws named (bare: `append_nil`, or applied: `append_nil(rest)`), every definition's equations, the datatypes' facts, logic, and linear arithmetic. Arithmetic means literals computed, sums collected, and comparisons whose sides differ by a literal decided. It also uses every known fact, each first simplified with the others. An `if` left open is split, and the goal closes when both branches close. The step fails if nothing changed. |
| `simplify_once(l)` | One rewrite with `l`: the first instance of its left side, every copy of it. |
| `unfold(f, g)` | Unfolds the definitions of `f` and `g` only. |
| `law(args)` | Uses a law: its conclusion finishes the goal. Its `requires` are met by the facts known: the law's own `requires`, steps proven before, the branch's conditions, and what simplifying them gives. Arguments left out are found by matching the goal. |
| `assert p by { steps }` | Proves `p`, then knows it for the rest of the proof. `assert p` alone proves it by the ladder. |
| `calc { a == b; _ == c }` | A chain of equations, each step proven by the ladder (which knows the facts in scope). It ends at the goal's two sides. |
| `witness(e)` | On `exists(x -> p)`: proves `p` at `e`. |
| `arithmetic()` | Linear integer arithmetic, by kernel steps, from the known facts. `len(xs) >= 0` holds, since `len` returns a `Natural`. |
| `nonlinear()` | Asks Z3 (strings, products of unknowns). The law is marked `[assumed: z3]`. |
| `compute()` | `simplify()`. |
| `auto()` | The ladder: `simplify()`, then `arithmetic()`, then `nonlinear()`. |

There are no steps for splitting `&&`, choosing a side of `||`, or closing `a == a`: `simplify()` and `auto()` do these.

## Goals

A goal prints its variables and facts, then `⊢` and what is left to prove:

```
  case cons
  x : T
  rest : List[T]
  ih : rest + [] == rest
  (`append_nil` may be called here on `rest`: it is `ih`)
  ⊢ [x] + rest + [] == [x] + rest
```

A `match` the goal can't unfold prints as `case_T(…)`; the error names the value it is stuck on. Split it with `match value { … }`.

## Errors

- `unsolved goals`: a branch's steps ended with the goal left.
- `no goals left for …`: a step after the goal was finished (often `arithmetic()` after a `simplify()` that already finished it).
- `simplify made no progress`: no rule applies.
- `omega: no contradiction found` (from `arithmetic()`): a fact is missing (assert it, or split a stuck `match`), or the goal isn't linear (`nonlinear()`).
- "`law` is being proven: it can be called only in an induction's branch": a recursive call on a value that isn't smaller.

## An example

The shape of a proof by cases and recursion: `match xs`, then `if x <= y`, else `match rest` with a nested `if x <= z`, and the recursive call on `[z] + more` in the last branch.

```
lemma insert_keeps_sorted(x: Int, xs: List[Int]) requires is_sorted(xs) ensures is_sorted(insert(x, xs)) {
  match xs {
    [] => simplify(),
    [y, ..rest] => if x <= y {
      simplify()
    } else {
      match rest {
        [] => auto(),
        [z, ..more] => if x <= z {
          auto()
        } else {
          assert is_sorted(insert(x, [z] + more)) by {
            insert_keeps_sorted(x, [z] + more)
          }
          simplify()
        },
      }
    },
  }
}
```
