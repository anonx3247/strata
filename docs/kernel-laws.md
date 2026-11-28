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
- `lemma NAME[TYPES](PARAMS) requires COND ensures STATEMENT { PROOF }`. The parameters are the law's variables, and each `requires` is an assumption. Nothing is introduced by hand: what a `for all` or `==>` would say is a parameter or a `requires`. A statement `p ==> q` is read as `requires p` and `holds q` (each conjunct of `p` an assumption, named `h`, `h1`, …), and a statement `!p` assumes `p` and proves `false`. Inside a proof, `assume()` does the same for a goal `p ==> q` or `!p` left by a step.
- No body: the law is proven by the ladder, `auto()`. A body `?proof`: the law is open.
- A function's `ensures` is the goal `f.ensures`, and an `impl`'s trait laws are goals `T.law`. A `lemma` with no `ensures` proves the goal its name spells: `twice_ensures` proves `twice.ensures`, and `impl_priority_irreflexive` proves `Priority.irreflexive` (an `impl` obligation takes the prefix `impl_`).
- A parameter of a refined type (`n: Natural`) brings the assumption `n >= 0`.
- `[A: Ord]` brings `A`'s `<` (`lt_A`, printed `a < b`) and Ord's laws as assumptions: `irreflexive : forall(a -> !(a < a))`, `transitive`, `total`, and the derived law `asymmetric : forall(a -> forall(b -> a < b ==> !(b < a)))`, proven once in core/ord.kp from the first two, which `simplify` uses like any fact. A law proven so holds for every `Ord`; if it can't be proven from those, it fails, and it never becomes an obligation. On an `Ord`, `a <= b` is `!(b < a)`; on `Int`, `simplify` writes `!(a < b)` as `b <= a`, so the condition `if x <= y` and a definition's `if !(y < x)` meet.
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
- A call of a computed function: `made(fs, v, b)(g)` (printed `made(fs, v, b, g)`).
- A type made by a type function (`Patch[User]`, with `type Patch[T: Record] = …` and a blanket `impl[T: Record]`) is a datatype like any record, in a module file outside a package (a package's module is read back from its text, where a made type's name doesn't parse). A field left out of a record takes its default. A `mut self` method is left out.
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
| `law(args)` | Uses a law, or an assumption that is one (`transitive(a, b, c)`): its conclusion finishes the goal. Its `requires` are met by the facts known: the law's own `requires`, steps proven before, the branch's conditions, and what simplifying them gives. Arguments left out are found by matching the goal. Any law can be used inside a step: `assert a < c by { transitive(a, b, c) }`. |
| `assume()` | A goal `p ==> q` (or `!p`) left by a step: `p` becomes a fact. |
| `assumption()` | Finishes the goal with a fact that is it. |
| `assert p by { steps }` | Proves `p`, then knows it for the rest of the proof. `assert p` alone proves it by the ladder. |
| `calc { a == b; _ == c }` | A chain of equations, each step proven by the ladder (which knows the facts in scope). It ends at the goal's two sides. |
| `extensionality()` | On `f == g` between functions: proves `f(x) == g(x)` for a new `x` (kernel steps: the equation under a lambda, then `eta` on both sides). |
| `witness(e)` | On `exists(x -> p)`: proves `p` at `e`. |
| `arithmetic()` | Linear integer arithmetic, by kernel steps. It first simplifies the facts and the goal (`Task { priority: Low, due: a } < Task { priority: Low, due: b }` becomes `a < b`), then refutes the goal's negation from them. `len(xs) >= 0` holds, since `len` returns a `Natural`. |
| `nonlinear()` | Asks Z3 (strings, products of unknowns). The law is marked `[assumed: z3]`. |
| `compute()` | `simplify()`. |
| `auto()` | The ladder: `simplify()`, then `arithmetic()`, then `nonlinear()`. First it applies the laws among the facts (`transitive`, an assumption like `forall(x -> p(x) ==> q(x))`) to the facts once. |

There are no steps for splitting `&&`, choosing a side of `||`, or closing `a == a`: `simplify()` and `auto()` do these.

Facts are the same wherever they come from: a law's `requires`, an `assert`, a branch's condition and a recursive call's result are all simplified with the others before `simplify`, `arithmetic` and `auto` use them.

A record pattern binds by field name, in any order, and `..` leaves out the rest: `Bin { right, left, .. }`.

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

A record prints by its fields (`User { name: u.name, age: u.age }`), and every record has the law `T { ..t } == t` built in, which `simplify` uses. A bounded function prints without its dictionary (`insert_sorted(x, xs)`), as it is written. A `match` the goal can't unfold prints as `match value { [] => …, [y1, ..more] => … }`; the error names the value. Split it with `match value { … }`, or use a lemma about it: a lemma's statement and the goal match once both are simplified.

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
