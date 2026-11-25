# Verification

`strata verify` proves a program's contracts with an SMT solver, runs the solver's counterexamples to confirm them, and fuzzes whatever it can't prove. It also says whether each function terminates. The examples below come from [`examples/math.strata`](../examples/math.strata); under each is what `strata --file examples/math.strata verify --checks` says about it.

Each function gets one of these results:

| | result | meaning |
|---|---|---|
| `✓` | proven | the solver proved every check |
| `~` | fuzzed | some checks couldn't be proven; generated inputs found no failure, and each unproven check says why |
| `✗` | failed | a counterexample, from the solver (confirmed by running it) or from the fuzzer |
| `○` | incomplete | the function still has a hole |

## Refinements

A refinement is a type with a constraint. Every place a value must satisfy one (a result, an argument, a field, an assignment) is a check.

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

The solver found `d = 9`, and running `next_digit(9)` confirmed it: a counterexample is only reported once it really fails.

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

The recursive call is checked against `factorial`'s own contract: `n - 1` is a `Natural` there (since `n != 0`), and the result is `Positive` because `n` and the callee's result are. Termination needs a *measure*, a number that goes down on every recursive call and is never negative. The verifier finds it on its own, trying each integer parameter, each list's length, and the differences in the function's comparisons:

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

When no single number goes down, a pair compared in order may, as for Ackermann's function:

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

A `for` loop over a list always ends. Here every assignment must keep `total` a `Natural`, its declared type:

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

A loop is verified from any state its variables can be in: the variables it changes take any values of their declared types, and one pass is checked from there. After a `while`, the state is that one with the condition false, or the state at one of its `break`s (a `break` leaves the loop with the condition still true). A `loop` is left only at a `break`; with none, the code after it is never reached. After a `for`, the variables it changed are any values of their types. A pass ends where its body ends or at a `continue`: what must hold after a pass is checked at each.

A `while` loop ends when a bound taken from its condition goes down on every pass. A property that holds *after* a loop usually needs a loop invariant: a fact true at the head of the loop every time. The verifier infers them. The candidates are:
- the condition's comparisons, made non-strict (`i < n` gives `i <= n`, which still holds after the last pass);
- the function's `ensures`, about the variable it returns;
- its `requires`;
- each integer the loop changes, compared with its value on entry (`x₀`), with 0, with the integer parameters, and with the other integers it changes.

Those that can be false on entry are dropped. Then, from any state where the others and the condition hold, one pass runs, and those that can be false after it (where its body ends, or at a `continue`) are dropped, until a pass drops nothing. What remains holds on entry and after every pass, so it is assumed at the head. A fact the declared types already give is left out. `--checks` lists the invariants found:

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

An invariant no candidate states is not found: a sum's closed form is one. Then the check is fuzzed, and the result says so, unless the loop states it. An `invariant e` clause, one per line between a loop's header and its body (as a function's `requires`), is proven on entry and after every pass, including at each `continue`, and then assumed at the head, with the inferred ones (each may need the others):

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

**Invariants found by search are written down.** Most loops' invariants are found at once (a few queries, a few thousand of the solver's resource units) and stay implicit. When finding them takes a real search (more resource units than a proof may use, 2M), `verify` writes what it found into the code as `invariant` clauses on that loop, so later runs only prove them and readers see them. It says so, a line per definition:

```text
edited quoted_end: added `invariant lx.at <= n` to `while lx.at < n && char_at(lx, lx.at) != "\""` (found by search in 0.6 s, 3M solver resource units)
```

All of a run's invariants are written in one edit, after its verdicts: only the module files of the changed definitions are encoded again (an `invariant` clause changes no signature, and `verify` has just proven it), and only clauses that print as Strata and read back as the same tree are written. In a package opened from its folder (`strata verify`, `strata build` without `--file`), the change is logged in `strata.log` as made by `verify` (`"by":"verify"`) and committed, as an edit is; with `--file` it is written as `--file F edit` writes. Only the program verified is written: std and core never are when they are dependencies. An invariant that speaks of a variable's value before the loop (`x₀`) is written without new syntax: as the expression the variable was set to just before the loop, when neither the loop nor the statements since change that expression's variables (`mut end := n` before the loop: `invariant end <= n`), else with a binding `verify` makes just before the loop, named apart from the function's names (`start_i := i`, then `invariant i >= start_i`; the message says so: ``added `invariant i >= start_i` to `while …` and `start_i := i` before it``). `--no-write` writes nothing. A loop with written invariants isn't searched: they are proven, and what else holds is asked only at a proof's size (0.5M units a question, giving up at the first unanswered).

**One search budget per run.** All of a run's loop-invariant searches (termination's and verification's) draw on one pool of the solver's resource units (deterministic, unlike time), 2,000M by default, about twice what all of the compiler's loops take with nothing cached (std's take 53M, the examples' under 0.1M). A search answered before in the run costs nothing. Once the pool is used up, the loops not yet searched are reported, and their verdicts aren't kept:

```text
note: `quoted_end`: the invariant search budget was used up before `while …` was searched: write an `invariant` for that loop, or verify again with a larger budget (`--search-budget N`; this run's was 0k solver resource units)
```

`--search-budget N` sets it (a negative N: no limit). Which loops are left when it runs out depends on the order the workers reach them; `--show-searches` lists what each loop's search took and found.

A written invariant is also checked when the program runs, at the loop's head: on entry, after every pass and `continue`, and when the loop ends normally (not at a `break`, whose state it needn't describe). `for` and `loop` take them too. A `for` loop's invariant can't name the loop's variable: it holds before the first element and after the last, where there is none.

Termination is never assumed. Whether the Collatz sequence always reaches 1 is an open problem, so:

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

Its checks are proven (every assignment keeps `x` a `Positive`); only termination is unknown. A function can be marked `terminates` to assume it, which is reported as assumed.

## Lists

A list is known by its length and by how its elements are made, so "every element satisfies P" can be proven:

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

The comprehension keeps the positives (a `filter`) and squares them (a `map`), so they stay positive. In `is_prime`, every `d` in `2..n` is at least 2, so `n % d` never divides by zero. `primes` keeps only numbers that passed `is_prime`, whose contract says they are at least 2. What is known of every element also flows in from a contract:

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

Positions are known too: `xs[k]`, a slice, `+`, `push`, `reverse` and a literal keep which element is where, so a property relating neighbours can be stated and kept:

```rust
type Sorted = List[Int] where all(1..len(it), i -> it[i - 1] <= it[i])

fn tail(xs: Sorted) -> Sorted
  requires len(xs) > 0 {
  xs[1..len(xs)]
}

fn add_max(xs: Sorted, x: Int) -> Sorted
  requires len(xs) == 0 || xs[len(xs) - 1] <= x {
  push(xs, x)
}

fn add(xs: Sorted, x: Int) -> Sorted {
  push(xs, x)
}
```

```text
✓ tail                     proven (1 check)
✓ add_max                  proven (1 check)
✗ add                      Refinement violation (found by the fuzzer)
    input:   [-952, 0, 1000], -676
    result of `add`: [-952, 0, 1000, -676] does not satisfy `Sorted` (…)
```

An element of a list of a checked type is a function of its position: its integers, strings and booleans are SMT functions of the position, so equal positions hold equal elements, and the list inside an element (a record's field) is a function of both positions. `all(xs, p)` and `all(lo..hi, i -> …)`, once known, become a fact quantified over the positions, which reaches the elements the code takes. A counterexample with a non-empty list is shown with up to 6 elements: the first elements of the parameters' lists are made when the function is, and asked for when the model has a non-empty list. Not yet: positions after `filter` and `sort` (only "some element of the list" is known there), and quantified facts nested inside one another (a conversation whose every tool result answers some earlier message), which the solver proves only by instantiating them by the thousand, past what a proof may cost.

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

A program checks its contracts when it runs: `requires`, `ensures`, refined types, `assert`, `invariant`, indexes. Once `verify` has proven a check, `strata build` and `strata run` leave it out of the compiled code, so proven code pays nothing for it. `build` first verifies what changed since it was last verified (what didn't is cached), so a build is as proven as it can be:

```text
$ strata --file t.strata build
verified 3 changed: 3 proven, 0 fuzzed
built t: 7 runtime checks left out (proven by `strata verify`), 0 kept
```

- **Each check counts on its own.** A function `verify` only fuzzed still loses the checks it proved; the others stay, as do all of a function with a counterexample.
- **A function's entry checks** (its parameters' types, its `requires`) are proven where it is called. They are left out when every call proved them: each function that refers to it proved the call, and nothing else refers to it (a contract, a type's refinement, a test, a trait's method). A single caller that doesn't prove the call keeps them, for everyone.
- **What a proof assumed still holds.** A proof may rest on another function's `ensures`, or on code `verify` can't see (outside code, a capability); that function's checks stay unless they are proven themselves, so a broken assumption faults where it breaks.
- **Only proofs of the code as it is.** `verify` keeps what it proved with each function's hash, with everything it depends on (`.strata/` in a package, `~/.cache/strata/scripts/` for a script, with its verdicts). `build` verifies what changed first; `run` (compiled in memory, to start fast) and `build --no-verify` use what was proven before, and a function changed since keeps all its checks.
- **`--all-checks`** compiles every check back in (`build`, `run`). The fuzzer and the cross-check always run with every check: they are how a mistake of the prover's shows.

Trait methods count too: their proofs are found under `verify`'s names for them (`Money.combine`). A proven division calls the runtime's division without its check for 0. The standard library is proven, all of it (the test `std_is_proven` keeps it so), so its precompiled library leaves out the checks inside its functions; those of their arguments and `requires` stay, since the calling program proves them or not. `--all-checks` compiles std with the program, every check kept. Other checks that run in C (a string's index, `split`'s separator) stay.

Every result, option and setting of `strata verify` is in the [verification reference](verification-reference.md).
