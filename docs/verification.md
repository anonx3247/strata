# Verification

`strata verify` proves a program's contracts with an SMT solver. It runs each counterexample the solver finds to confirm it, and fuzzes whatever it can't prove. It also reports whether each function terminates. The examples below come from [`examples/math.strata`](../examples/math.strata), each followed by the output of `strata --file examples/math.strata verify --checks`.

Each function gets one of these results:

| | result | meaning |
|---|---|---|
| `✓` | proven | the solver proved every check |
| `~` | fuzzed | some checks couldn't be proven, and generated inputs found no failure. Each unproven check gives the reason |
| `✗` | failed | a counterexample, from the solver (confirmed by running it) or from the fuzzer |
| `○` | incomplete | the function still has a hole |

## Refinements

A refinement is a type with a constraint. Every place where a value must satisfy one (a result, an argument, a field, an assignment) is a check.

```rust
type Digit = Int where 0 <= it && it <= 9

/// Wrong: a digit plus one isn't always a digit.
fn next_digit(d: Digit) -> Digit {
  d + 1
}
```

```text
✗ next_digit               Refinement violation (found by the solver)
    input:   9
    result of `next_digit`: 10 does not satisfy `Digit` (where 0 <= it && it <= 9)
```

The solver found `d = 9`, and running `next_digit(9)` confirmed it. A counterexample is reported only after it fails when run.

## Refinements and contracts

A parameter's refinement behaves like a `requires`: it is assumed on entry and checked for each argument at the call. A result's refinement behaves like an `ensures`: it is proven at each `return` and assumed by callers. A fact about one value is therefore written as a refinement, and contracts are for facts that relate values (`requires lo <= hi`, `ensures it >= price`). `strata check` warns about a `requires` that names one parameter, an `ensures` that names only `it` (or only one element of a tuple result), and a refinement that names another parameter. `strata fix` rewrites each one (see [the language](language.md#refinements-and-contracts-one-way-each)). The rewrite doesn't change any verdict, since the same checks are made in their new places.

Named results (`-> (q: Natural, r: Natural)`) are the tuple's elements in an `ensures`. `verify` reads `q` as `it.0` and `r` as `it.1`, and reports a broken `ensures` in those terms (`ensures it.0 * b + it.1 == a`).

## Values before and after: `old`

A `mut` parameter, or `mut self`, changes during the call, so an `ensures` about it relates two values. In the clause, its name means its value when the function returns, and `old e` means `e` evaluated when the function started ([the language](language.md#refinements-and-contracts-one-way-each)). `verify` saves the parameters' initial values and proves the clause at every exit. At a call, it reads the callee's `ensures` with the arguments' values before the call for `old` and the callee's final values for the rest, so callers can build on it:

```rust
fn bump(mut n: Int)
  ensures n == old n + 1 {
  n += 1
}

fn bump_twice(mut n: Int)
  ensures n == old n + 2 {
  bump(mut n)
  bump(mut n)
}

fn push_positive(mut xs: List[Int], x: Int)
  ensures len(xs) == old len(xs) + 1 {
  if x > 0 {
    xs += [x]
  }
}
```

```
✓ bump                     proven (1 check)
✓ bump_twice               proven (1 check)
✗ push_positive            Ensures violation (found by the solver)
    input:   [], 0
    postcondition of `push_positive` violated: ensures len(xs) == old len(xs) + 1 (before: len(xs) = 0, after: len(xs) = 0) (stack: push_positive)
```

Trait method contracts work the same way. Given `fn incr(mut self) ensures self.total() == old self.total() + 1` in a trait `Counter`, a generic `fn incr_twice[C: Counter](mut c: C) ensures c.total() == old c.total() + 2` that calls `mut c.incr()` twice is proven. A method known only by its contract is assumed to return the same result for the same values.

When an `ensures` with `old` remains a run-time check, the program evaluates each `old e` on entry and keeps it until the check, so a failure can show both values. Keeping the value forces an in-place update of it to copy, so this happens only for checks that `verify` didn't prove.

## Trait models

A trait's `model` ([the reference](language-reference.md#traits)) is the abstract state its contracts describe. Each implementation defines it for its type with `model name = e`. `verify` proves two things separately:

- **Each implementation** meets the trait's contracts, with the model replaced by its definition. `MemDb.save` is proven to make `self.all` equal to `old self.all.with(id, o)`. An implementation that keeps only the last saved order (`model orders = match self.last { Some((k, o)) => Dict { k: o }, None => Dict {} }`) is refuted with a counterexample: one order already stored, a second one saved, two entries before and one after.
- **Code that uses the trait** reasons only through the model. A model is an unknown value of its type and a function of the trait value it is read from. The same value gives the same model wherever it came from, for example the joined branches of an `if`, or `old db` when nothing changed it. The model is constrained by the invariants and the methods' contracts. `fn ship(mut db: Db, id: Int) -> Result[(), String]`, which looks up the order and saves it as shipped, is proven to ensure `match old db.orders.get(id) { Some(o) => it is Ok && db.orders == (old db.orders).with(id, Order { ..o, shipped: true }), None => it is Err && db.orders == old db.orders }`.

A trait's `invariant` refines the model of every implementing type. A value of the trait's type carries it as a refinement: a function taking `db: Db` knows it, and the new value of a `mut` argument is assumed to satisfy it. Each implementation's methods assume it for `self` and other `Self` parameters. They must preserve it in a `mut self` method and establish it for a result of type `Self`. A trait's `ensures` is proven in each `mut self` method.

Contracts that name a model are assumed contracts (`(assumed: not checked when the program runs)`), because computing the abstraction at run time is what the implementation exists to avoid. One that `verify` neither proves nor refutes makes its function assumed (`≈`).

## State graphs

`strata view --graph T` uses the solver to find how values of the sum type `T` move between variants. For each function that takes and returns a value holding `T`, it asks whether the function can move a value from one variant to another, and draws the moves that may happen. The result is the type's pipeline, generated from the code instead of declared.

- Each question is an `ensures !(x is A && it is B)` on a copy of the function.
- Through a list, the question is `!(graph_i < len(old xs) && graph_i < len(xs) && old xs[graph_i].status is A && xs[graph_i].status is B)`, with the position as a parameter of the copy. Through a dict, it is the same at a key `graph_key`.
- Questions settled by the body's shape are skipped: a list returned unchanged, or a call to a function that moves the type.
- Questions the solver leaves unknown are run on generated inputs.

Moves proven impossible are left out, so the graph shows what the code can do. [The tools](tools.md#state-graphs-how-a-types-values-move) page shows the graph, along with the moves of a run and of a diff.

## Laws of statements and shared traits

A law can be a sequence of calls that ends in the condition that must hold ([the reference](language-reference.md#traits)). `verify` checks it for each implementation as a function whose body is the law and whose result must be `true`. Its `Self` parameters are assumed to meet the trait's invariants, and the calls go through the implementation's methods.

In `tests/cases/cli-shared_traits_and_laws`, a queue of task ids (`model pending: Set[Int]`, `model claimed: Set[Int]`) has an in-memory implementation. `verify` proves `enqueue_once` (enqueuing twice adds once) and `no_double_claim` (claiming twice succeeds at most once), and refutes `claim_after_enqueue` with the values and the steps:

```text
✗ law claim_after_enqueue for MemQueue Law violation (found by the solver)
    input:   MemQueue { waiting: Set[Int] {}, taken: Set[Int] { 21 } }, 21
    law `claim_after_enqueue` does not hold for `MemQueue` with q = MemQueue { waiting: Set[Int] {}, taken: Set[Int] { 21 } }, id = 21: after mut r := q; mut r.enqueue(id); mut r.claim(id)
```

Callers can't use proven laws as facts. Code that uses the trait sees only its contracts and invariants.

The queue is a `shared trait`, used by many workers at once. Its invariants hold under every interleaving, because each method is one atomic step that preserves them from any state where they hold. For calling code, each call on a shared value starts from a model that may be any value the invariants allow, as if another worker had just acted. As a result:

- `fn added(mut q: Queue, id: Int) -> Int where it == 0 || it == 1`, which uses only the results of two `enqueue` calls, is proven.
- `fn enqueued_now(mut q: Queue, id: Int) -> Int ensures q.pending.contains(id) || q.claimed.contains(id)` is proven, since what the last call ensures holds until the next call on `q`.
- `fn second_adds_nothing(mut q: Queue, id: Int) -> Int where it == 0`, which relies on the first `enqueue`'s effect during the second, is not proven. The reason says why: "`q` when `enqueue` is called: `Queue` is shared, so another worker may have changed it since the last call (only the trait's invariants are known)".

Inside an implementation, a call on its own field (`self.db`) keeps its facts, because the method is a single step. `check` enforces this: at most one call per path may change the outside state (see [the reference](language-reference.md#traits)). A value of a concrete implementing type (`MemQueue`) is not shared, and its code runs alone.

## Recursion and termination

```rust
/// n! is never 0.
fn factorial(n: Natural) -> Positive {
  if n == 0 { 1 } else { n * factorial(n - 1) }
}
```

```text
✓ factorial                proven (2 checks)
    proven:  argument `n` of `factorial`: Natural
    proven:  result of `factorial`: Positive
    terminates: `n` goes down on every recursive call and is never negative there
```

The recursive call is checked against `factorial`'s own contract. `n - 1` is a `Natural` there, since `n != 0`, and the result is `Positive` because both `n` and the recursive result are. Termination needs a measure: a number that decreases on every recursive call and is never negative. The verifier finds one by trying each integer parameter, each list's length, and the differences in the function's comparisons:

```rust
fn gcd(a: Natural, b: Natural) -> Natural
  ensures a > 0 || b > 0 ==> it > 0 {
  if b == 0 { a } else { gcd(b, a % b) }
}
```

```text
✓ gcd                      proven (5 checks)
    proven:  no division by zero
    proven:  argument `a` of `gcd`: Natural
    proven:  argument `b` of `gcd`: Natural
    proven:  result of `gcd`: Natural
    proven:  ensures a > 0 || b > 0 ==> it > 0
    terminates: `b` goes down on every recursive call and is never negative there
```

When no single number decreases, a pair compared lexicographically may, as in Ackermann's function:

```rust
fn ackermann(m: Natural, n: Natural) -> Positive {
  if m == 0 {
    n + 1
  } else {
    if n == 0 { ackermann(m - 1, 1) } else { ackermann(m - 1, ackermann(m, n - 1)) }
  }
}
```

```text
✓ ackermann                proven (7 checks)
    terminates: the pair (`m`, `n`), compared in order, goes down on every recursive call and is never negative there
```

## Loops

A `for` loop over a list always terminates. In this example, every assignment must keep `total` a `Natural`, its declared type:

```rust
fn triangle(n: Natural) -> Natural {
  mut total: Natural = 0
  for i in 1..=n {
    total += i
  }
  total
}
```

```text
✓ triangle                 proven (3 checks)
    proven:  `let` annotated `Natural`
    proven:  assigning `total`: Natural
    proven:  result of `triangle`: Natural
    terminates: no recursion, and only `for` loops
```

**How a loop is verified.** The variables a loop changes are given arbitrary values of their declared types, and one pass is checked from that state.

- After a `while`, the state is that state with the condition false, or the state at one of its `break`s, where the condition may still be true.
- A `loop` exits only at a `break`. Without one, the code after it is unreachable.
- After a `for`, the variables it changed have arbitrary values of their types.
- A pass ends at the end of the body or at a `continue`, and what must hold after a pass is checked at each.

A `while` loop terminates when a bound derived from its condition decreases on every pass. A property that must hold after a loop usually needs a loop invariant, a fact true at the head of the loop every time.

**Invariants derived from the code.** The verifier first reads invariants off the loop's code, as a compiler finds induction variables. They hold by construction, so they are assumed without a search. `x₀` is `x`'s value on entry:

- An integer that every pass changes by the same amount on every path, including at a `continue` (`i += 1`, `j -= 2`), is an induction variable. Two of them keep a linear relation, such as `i - j == i₀ - j₀` or `2 * i - j == 2 * i₀ - j₀`.
- An integer that every assignment in the loop only increases keeps `x >= x₀`, through branches and inner loops. The amount can be a literal or a length (`len`, `byte_len`). It is the other way round for one that only decreases.
- With `while i < n`, where the loop doesn't change `n` and `i` goes up by at most 1 a pass, `i <= n` holds if it holds on entry. One question to the solver checks that. If the answer is no, `i <= n || i == i₀` holds anyway. `<=`, `>`, `>=` and `!=` are handled the same way.
- A variable given only literals (or itself) is one of them or its value on entry. One given only values of at least `l` (literals, lengths, itself plus a non-negative amount) keeps `x >= l` if that holds on entry, and the same goes for an upper bound.
- A list, dict or set that the loop only pushes to or inserts into doesn't get shorter (`len(xs) >= len(xs₀)`). One that it only pops or removes from doesn't get longer.

A variable that the loop doesn't change keeps its value without any invariant. A place passed `mut`, assigned a value of another form, or named again by a binding inside the loop gets none of these facts. Neither does a sized integer, since sized integers wrap. The same facts hold at the head of a `for` and a `loop`.

**A search, only where a check needs it.** When a check made after the start of a loop isn't proven with the derived invariants, `verify` runs that function again with a search for more invariants. Such a check can be an index, an `ensures`, an overflow, a written `invariant` or a termination bound. The search uses these candidates:

- the condition's comparisons made non-strict (`i < n` gives `i <= n`, which still holds after the last pass)
- the function's `ensures`, about the variable it returns
- its `requires`
- each integer the loop changes, compared with its value on entry, with 0, with the integer parameters, and with the other integers the loop changes

At first only the candidates that name a variable of the failing checks or of the loop's condition are tried. All of them are tried only if a check still fails. Candidates that can be false on entry are dropped. Then one pass runs from any state where the remaining candidates, the derived invariants and the condition hold. Candidates that can be false after the pass (at the end of the body or at a `continue`) are dropped. This repeats until a pass drops nothing. The remaining candidates hold on entry and after every pass, so they are assumed at the head. Facts already given by the declared types are omitted. `--checks` lists the invariants, derived and found:

```rust
fn isqrt(n: Natural) -> Natural
  ensures it * it <= n {
  mut r: Natural = 0
  while (r + 1) * (r + 1) <= n {
    r += 1
  }
  r
}
```

```text
✓ isqrt                    proven (6 checks)
    proven:  `let` annotated `Natural`
    proven:  assigning `r`: Natural
    proven:  result of `isqrt`: Natural
    proven:  ensures it * it <= n
    proven:  invariant of `while (r + 1) * (r + 1) <= n`: it * it <= n (it = r) (inferred)
    proven:  invariant of `while (r + 1) * (r + 1) <= n`: r <= n (inferred)
    terminates: `while (r + 1) * (r + 1) <= n` ends: `n - (r + 1) * (r + 1) + 1` goes down on every pass
```

An invariant that no candidate states, such as the closed form of a sum, isn't found. The check is then fuzzed and the result says so, unless the loop states the invariant. An `invariant e` clause goes on its own line between the loop's header and its body, like a function's `requires`. It is proven on entry and after every pass, including at each `continue`, and then assumed at the head together with the inferred invariants, since each may depend on the others:

```rust
fn triangle_closed(n: Natural) -> Natural
  ensures 2 * it == n * (n + 1) {
  mut total: Natural = 0
  mut i: Natural = 0
  while i < n
    invariant 2 * total == i * (i + 1) {
    i += 1
    total += i
  }
  total
}
```

```text
✓ triangle_closed          proven (9 checks)
    proven:  `let` annotated `Natural`
    proven:  `let` annotated `Natural`
    proven:  invariant `2 * total == i * (i + 1)` on entry to `while i < n`
    proven:  assigning `i`: Natural
    proven:  assigning `total`: Natural
    proven:  invariant `2 * total == i * (i + 1)` after a pass of `while i < n`
    proven:  result of `triangle_closed`: Natural
    proven:  ensures 2 * it == n * (n + 1)
    proven:  invariant of `while i < n`: i <= n (inferred)
    terminates: `while i < n` ends: `n - i` goes down on every pass
```

**Invariants found by search are written into the code.** Most loops' invariants are found quickly (a few queries, a few thousand solver resource units) and stay implicit. When finding them takes a real search, more than the 2M resource units a single proof may use, `verify` writes them into the code as `invariant` clauses on that loop. Later runs then only have to prove them, and readers can see them. `verify` prints one line per definition:

```text
edited quoted_end: added `invariant lx.at <= n` to `while lx.at < n && char_at(lx, lx.at) != "\""` (found by search in 0.6 s, 3M solver resource units)
```

- All of a run's invariants are written in one edit, after its verdicts. Only the module files of the changed definitions are re-encoded, since an `invariant` clause changes no signature and `verify` has just proven it. Only clauses that print as Strata and read back as the same tree are written.
- In a package opened from its folder (`strata verify`, or `strata build` without `--file`), the change is logged in `strata.log` as made by `verify` (`"by":"verify"`) and committed, like an edit. With `--file`, it is written the way `--file F edit` writes.
- Only the program being verified is written. std and core are never written when they are dependencies.
- An invariant that refers to a variable's value before the loop (`x₀`) is written without new syntax. If neither the loop nor the statements since change the variables of the expression the variable was last set to, that expression is used: with `mut end := n` before the loop, the invariant is `invariant end <= n`. Otherwise `verify` adds a binding just before the loop, with a name distinct from the function's names (`start_i := i`, then `invariant i >= start_i`). The message says so: ``added `invariant i >= start_i` to `while …` and `start_i := i` before it``.
- `--no-write` writes nothing.
- A loop with written invariants isn't searched. Its invariants are proven, and any further facts are asked only within a single proof's budget: 0.5M units per question, stopping at the first unanswered one.

**One search budget per run.** All of a run's loop-invariant searches, for termination and for verification, draw on one pool of solver resource units, which are deterministic, unlike time. The default pool is 2,000M units, about twice what all of the compiler's loops take with nothing cached. std's loops take 53M, and the examples' less than 0.1M. A search already answered in the same run costs nothing. Once the pool is used up, the loops not yet searched are reported and their verdicts aren't kept:

```text
note: `quoted_end`: the invariant search budget was used up before `while …` was searched: write an `invariant` for that loop, or verify again with a larger budget (`--search-budget N`; this run's was 0k solver resource units)
```

`--search-budget N` sets the pool; a negative N means no limit. Which loops are left when the pool runs out depends on the order the workers reach them. `--show-searches` lists what each loop's search cost and found.

A written invariant is also checked at run time at the loop's head: on entry, after every pass and `continue`, and when the loop ends normally. It isn't checked at a `break`, whose state it doesn't have to describe. `for` and `loop` accept invariants too. A `for` loop's invariant can't name the loop variable, because it must hold before the first element and after the last, where the variable has no value.

Termination is never assumed. Whether the Collatz sequence always reaches 1 is an open problem, so this loop's termination stays unproven:

```rust
fn collatz_steps(n: Positive) -> Natural {
  mut x: Positive = n
  mut steps: Natural = 0
  while x != 1 {
    if x % 2 == 0 { x /= 2 } else { x = 3 * x + 1 }
    steps += 1
  }
  steps
}
```

```text
✓ collatz_steps            proven (6 checks)
    may not terminate: the loop `while x != 1`: no bound was shown to go down (tried `x - 1`, `1 - x`)
```

Its checks are proven, since every assignment keeps `x` a `Positive`; only termination is unknown. Marking a function `terminates` assumes termination, and the result reports it as assumed.

A loop with no single decreasing bound is still handled when its measures are bounds from its condition or from the comparisons its body tests. They are tried alone, then in pairs compared lexicographically (the first decreases, or it stays the same and the second decreases):

```rust
fn grid(n: Int, m: Int) -> Int {
  mut i := 0
  mut j := 0
  mut count := 0
  while i < n {
    if j < m {
      j += 1
      count += 1
    } else {
      i += 1
      j = 0
    }
  }
  count
}
```

```text
    terminates: `while i < n` ends: the pair (`n - i`, `m - j`), compared in order, goes down on every pass
```

Otherwise the measure can be written as a `decreases` clause: on a `while` or `loop`, on its own line between the header and the body (like `invariant`), or on a recursive function next to `requires`. It is an `Int` expression, or a tuple of them compared lexicographically, and must be non-negative where it decides termination. `verify` proves that it decreases at the end of every pass, or at every recursive call, and uses it for termination. Where it isn't proven, it is checked at run time, so a wrong measure stops the program with a fault instead of looping forever. A counterexample is confirmed by running it:

```rust
fn drain(a0: Natural, b0: Natural) -> Int {
  mut a := a0
  mut b := b0
  mut steps := 0
  while a + b > 0
    decreases (a, b) {
    if a > 0 {
      a -= 1
      b = b0
    } else {
      b -= 1
    }
    steps += 1
  }
  steps
}

fn walk(a: Natural, b: Natural) -> Int
  decreases (a, b) {
  if b > 0 {
    1 + walk(a, b - 1)
  } else if a > 0 {
    1 + walk(a - 1, 3)
  } else {
    0
  }
}
```

```text
    proven:  decreases `(a, b)` after a pass of `while a + b > 0`
    terminates: `while a + b > 0` ends: its `decreases (a, b)` measure goes down on every pass
    proven:  decreases `(a, b)` at a call of `walk`
    terminates: its `decreases (a, b)` measure goes down on every recursive call and is never negative there
```

With `decreases (b, a)` instead, and `b = b0 + 1` where `a` decreases, the pass that lowers `a` raises `b`:

```text
✗ drain                    Assert violation (found by the solver)
    input:   1, 0
    decreases failed: `(b, a)` did not go down in a pass of `while a + b > 0` (before: 0, 1, after: 1, 0) (stack: drain)
```

## Lists

The verifier tracks a list's length and how its elements were produced, so it can prove that every element satisfies a property:

```rust
fn positive_squares(xs: List[Int]) -> List[Positive] {
  [x * x for x in xs where x > 0]
}

fn is_prime(n: Int) -> Bool
  ensures it ==> n >= 2 {
  n >= 2 && all(2..n, d -> n % d != 0)
}

fn primes(limit: Natural) -> List[Int where it >= 2] {
  (0..=limit).filter(is_prime)
}
```

```text
✓ positive_squares         proven (1 check)
✓ is_prime                 proven (2 checks)
    proven:  no division by zero
    proven:  ensures it ==> n >= 2
✓ primes                   proven (1 check)
```

The comprehension keeps the positive elements (a `filter`) and squares them (a `map`), so the results stay positive. In `is_prime`, every `d` in `2..n` is at least 2, so `n % d` never divides by zero. `primes` keeps only numbers that passed `is_prime`, whose contract says they are at least 2. Facts about every element can also come from a contract:

```rust
fn first_or_one(xs: List[Int]) -> Positive
  requires all(xs, x -> x > 0) {
  match xs {
    [x, ..] => x,
    [] => 1,
  }
}
```

```text
✓ first_or_one             proven (1 check)
```

Positions are tracked too. `xs[k]`, slices, `+`, `push`, `reverse` and list literals preserve which element is where, so a property relating neighbouring elements can be stated and preserved:

```rust
type Sorted = List[Int] where all(1..len(it), i -> it[i - 1] <= it[i])

fn tail(xs: Sorted) -> Sorted
  requires len(xs) > 0 {
  xs[1..len(xs)]
}

fn add_max(xs: Sorted, x: Int) -> Sorted
  requires len(xs) == 0 || xs[len(xs) - 1] <= x {
  xs + [x]
}

fn add(xs: Sorted, x: Int) -> Sorted {
  xs + [x]
}
```

```text
✓ tail                     proven (1 check)
✓ add_max                  proven (1 check)
✗ add                      Refinement violation (found by the fuzzer)
    input:   [-952, 0, 1000], -676
    result of `add`: [-952, 0, 1000, -676] does not satisfy `Sorted` (…)
```

**How lists are encoded.**

- An element of a list with a checked type is modelled as a function of its position. Its integers, strings and booleans are SMT functions of the position, so equal positions hold equal elements. A list inside an element (a record field) is a function of both positions.
- `all(xs, p)` and `all(lo..hi, i -> …)`, once known, become a fact quantified over positions, which applies to the elements the code reads.
- A counterexample with a non-empty list shows up to 6 elements. The first elements of the parameters' lists are created with the function and requested when the model has a non-empty list.
- Nested quantified facts are encoded by polarity. An example is a conversation where every tool result answers a call in some earlier message: `all(0..len(it), i -> … any(0..i, j -> any(calls, c -> c == id)))`. An `all` that is only ever known to be true gives its fact for every position. One that is only ever proven gives only what proves it: a position where it fails, or otherwise that it holds. An `any` inside a known `all` therefore becomes a witness position for the outer `all`, never a quantifier inside a quantifier. Appending calls and their results, or appending a result for a call that a `requires` refers to, is proven in milliseconds.
- Not supported: positions after `filter` and `sort`, where only "some element of the list" is known, and a witness shifted by a concatenation in front of it (`a + b` of two such conversations).

## Bugs the solver finds

```rust
/// Wrong: an empty list has no average.
fn average(xs: List[Int]) -> Int {
  sum(xs) / len(xs)
}
```

```text
✗ average                  Runtime violation (found by the solver)
    input:   []
    division by zero (stack: average)
```

## What is proven isn't checked again

A program checks its contracts at run time: `requires`, `ensures`, refined types, `assert`, `invariant` and indexes. Once `verify` has proven a check, `strata build` and `strata run` leave it out of the compiled code, so proven code doesn't pay for it. `build` first verifies whatever changed since it was last verified, using cached results for the rest, so each build is as proven as possible:

```text
$ strata --file t.strata build
verified 3 changed: 3 proven, 0 fuzzed
built t: 7 runtime checks left out (proven by `strata verify`), 0 kept
```

- **Each check counts separately.** A function that `verify` only fuzzed still drops the checks it proved and keeps the others. A function with a counterexample keeps all its checks.
- **Entry checks** (parameter types, `requires`) are proven at each call site. They are dropped when every call proved them: every function that refers to it proved its call, and nothing else refers to it (a contract, a type's refinement, a test, a trait method). If even one caller doesn't prove the call, the checks stay for every caller.
- **Assumptions keep their checks.** A proof may rest on another function's `ensures`, or on code `verify` can't see (outside code, a capability). That function's checks stay unless they are proven themselves, so a broken assumption faults where it breaks.
- **Only proofs of the current code count.** `verify` stores each proof with the function's hash and the hashes of everything it depends on, in the cache (`verify-strata/proofs/`; see the [verification reference](verification-reference.md#caching)). `build` verifies what changed first. `build --no-verify` uses earlier proofs, and a function changed since then keeps all its checks.
- **`run` doesn't wait for proofs.** If the program changed since it was last verified, it starts immediately with every unproven check, while `verify` runs alongside in a separate, lower-priority process. When `verify` finishes, the program compiled without the proven checks is swapped in while it runs. Each function whose code changed gets the new version, and every later call uses it; a call already in progress finishes in the old version. The verdicts are kept, so the next run starts with the proven code. Notes go to stderr, one at the start and a summary when the program ends:

  ```text
  $ strata run t.strata
  note: verifying alongside: the program starts with every check, and the proven code is swapped in while it runs
  ...
  note: verified alongside (verified 2 changed: 2 proven, 0 fuzzed): 1 function swapped to its proven code while running
  ```

  A check that fails before the proof arrives still fails as a fault, and any counterexample appears in the summary. By default, only a run with stderr on a terminal verifies alongside, so a script reading a program's output leaves no verifier running. `--verify-alongside` forces it, `--no-verify` disables it, and `--wait-for-proofs` verifies first, as `build` does. Calls go through swappable slots only in runs with a verifier alongside. Other runs, and every built executable, call functions directly.
- **`--all-checks`** compiles every check back in (`build`, `run`). The fuzzer and the cross-check always run with every check, because they are how a prover mistake would show up.

Trait methods are covered too; their proofs are stored under `verify`'s names for them (`Money.combine`). A proven division calls the runtime's division without its zero check.

The whole standard library is proven, and the test `std_is_proven` keeps it that way. Its precompiled library therefore leaves out the checks inside its functions. Checks on their arguments and `requires` remain, because each calling program proves them or doesn't. `--all-checks` compiles std with the program and keeps every check. Checks that run in C (a string index, `split`'s separator) always remain.

### Assumed contracts: trusted, never checked

An unproven contract remains a run-time check, and some checks cost too much to keep. Examples are an invariant of a whole data structure checked at every call, or a loop invariant that walks a list on every pass and makes the loop quadratic. Writing `assumed` before the clause leaves it to `verify` alone:

```strata
fn execute(mut b: Book, o: Order) -> List[Event]
  assumed requires book_ok(b)
  assumed ensures book_ok(b)
  ensures len(it) >= 1
{
  for l in b.levels
    assumed invariant sorted(b.levels)
  {
    …
  }
}
```

A proven contract is already dropped from the compiled code, so `assumed` only matters for contracts that `verify` doesn't prove. It marks the contract as trusted instead of checked, which is `verify`'s **assumed** level (`≈`), the same level as capability code and `extern c`.

- `verify` handles an assumed contract like any other. It proves it, and the function is then proven, or refutes it with a counterexample, which is a failure. Its checks are marked `(assumed: not checked when the program runs)`. If it is neither proven nor refuted (fuzzed without a failure), the function is reported as assumed rather than fuzzed, naming the contract: `≈ f  assumed: not proven: ensures … (assumed: trusted, never checked when the program runs)`. `--require proven` accepts it, as it accepts other assumed code, and lists it.
- An assumed `requires` is taken as true inside the function and must be proven at its calls.
- The compiled program never checks an assumed contract, proven or not. `strata build` warns about the assumed contracts that `verify` didn't prove (`N assumed contracts aren't proven: …`) and continues.
- `strata run --check-assumed` and `strata test --check-assumed` compile them in, to check them on real runs. The fuzzer always checks them.
- An assumed contract stays a clause. `check` doesn't suggest turning one that constrains a single parameter into a refinement, since refinements are checked at run time.

`strata build` also reports a written contract that remains a costly run-time check: an unproven loop `invariant` (O(n) per pass when it walks a list), or a `requires` or `ensures` that walks a list (`all`, `any`, `sum`, `filter`, a comprehension). It suggests proving the contract or marking it assumed. In the order book benchmark, marking its book-wide invariants assumed reduced a run of 32,000 commands from 18.1 s to 2.5 s.

The [verification reference](verification-reference.md) lists every result, option and setting of `strata verify`.
