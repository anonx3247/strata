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
    input:   [9]
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

A `while` loop ends when a bound taken from its condition goes down on every pass. A property that holds *after* a loop usually needs a loop invariant, which the verifier doesn't infer yet: then the check is fuzzed, and the result says so.

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
~ isqrt                    fuzzed: 200 cases, 0 discarded by `requires`; 3 of 4 checks proven
    not proven: ensures it * it <= n: the solver found a possible counterexample: n = 0, but running it did not fail
    terminates: `while (r + 1) * (r + 1) <= n` ends: `n - (r + 1) * (r + 1) + 1` goes down on every pass
```

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

## Bugs the solver finds

```rust
/// Wrong: an empty list has no average.
fn average(xs: List[Int]) -> Int {
  sum(xs) / len(xs)
}
```

```text
✗ average                  Runtime violation (found by the solver)
    input:   [[]]
    division by zero (stack: average)
```

## Reference

`strata verify` turns every contract into proof obligations and gives each function one of these results:

- **proven** (`✓`): the SMT solver proved every check. `--checks` lists them.
- **fuzzed** (`~`): some checks could not be proven, so the function was run on generated inputs and nothing failed. Each unproven check says why, for example *depends on `fold` is not modelled*.
- **failed** (`✗`): a counterexample. When the solver finds one, it is run in the interpreter first, and reported only if it really fails. Otherwise the fuzzer looks for one.
- **incomplete** (`○`): the function still has a hole.
- **assumed** (`≈`): trusted, not checked. Today these are the standard library's real terminal, disk and process (the `impl`s behind `Console`, `Files` and `Environment` that only `strata run` creates): their bodies call the system, so `verify` never runs them. Their contracts are taken as true, and callers are still checked against those contracts as usual.

Derived code is checked as well: `derive Json for Signup` makes `Signup.encode` and the rest, and also copies of `std.json`'s helpers made for `Signup`'s fields (`std.json.encode_as.for.Signup.4`), which are listed and checked with it, so a fault that only some field types can cause is reported there (with `integers = "proven"` under `[json]`, an `Int` field that may be beyond what JSON holds exactly).

The checks are exactly the interpreter's runtime checks: a callee's parameter refinements and `requires`, the refinements of constructed values and assigned fields, annotated bindings, `assert`, division by zero, and the function's own result type and `ensures`. Calls are checked against the callee's contract, never its body, which is how modules meet. A function passed by name (`lines.map(line_total)`) is checked against one arbitrary element of the list.

How it works (`src/smt.rs`): the body is run symbolically, with values as SMT terms. Records and sums become structures of terms, a list is known by its length and by how its elements are made (a checked type, a literal, `range`, `filter`, `map`, `+`, `push`, a part of another list), so `all(xs, p)` is proven on an arbitrary element and, once known, gives `p` to every element the code looks at, a value of a union (`Square | Circle`) is a tag saying which member's shape it has plus a value for each member (a member value passed where the union is expected, or branches returning different members, become one; a `match` arm narrows it to its member), branches are merged with `ite`, and a loop forgets the variables it changes. Each check becomes the question "can the negation be true under what is known here?", which is sent to the solver (Z3, or cvc5) as SMT-LIB text (`verify --smt` prints the queries, termination ones included). `unsat` means proven; `sat` comes with a model, which becomes the counterexample. Anything not modelled (floats, dicts, sets, list contents, most higher-order builtins, unions whose members can share a shape such as `List[Int] | List[String]` or include a type parameter) becomes an unknown value. That can only make a proof fail, never make a false proof succeed, and the reason is kept for the report. `Int` is a mathematical integer, as it is when the code runs.
Strings are sequences of characters (code points), as `chars()` counts them. These string builtins are modelled exactly: `len`, `+`, `contains`, `starts_with`, `ends_with`, `slice`, `get`, `chars` (its length), `index_of`, `replace` (every match, and a check that the pattern is not empty, which is a runtime fault), `to_string` of an `Int`, `Bool` or `String`, and `is_digit`. `parse_int` is exact for digits with an optional `-`, and for strings without a digit (`None`). Other forms (whitespace, `+`, `_`) are unknown. `trim` keeps only a length bound. `to_upper`, `to_lower`, `is_alpha`, `ord`, `chr`, `split`, `lines`, `repeat` and `reverse` are unknown. `matches(s, "regex")` becomes a regular-expression constraint with the interpreter's meaning. Strata regexes are Unicode regexes: `\d` is any decimal digit, `\w` and `\s` are Unicode classes, `\p{..}` works, `.` is any character but `\n`, and `(?i)` is Unicode case folding. The pattern is parsed the way the interpreter parses it, and translated exactly: literals, classes, groups, alternation, repetition, and `^` / `$` at the very start or end. Word boundaries, multi-line anchors and anchors elsewhere stay unknown. Z3 can't decide classes with hundreds of ranges (`\w`, `\p{L}`, Unicode `\d`), so such a class is bounded from both sides: `matches` is true on a subset of its ranges and false outside a superset of them. When one string is matched against two patterns, how their languages relate (one inside the other, or disjoint) is worked out exactly from the patterns' automata and added as a fact. So `\d{3}` implies `\d+`, and `[a-z]+` implies `\w+`.

**Strings past U+2FFFF.** SMT-LIB strings hold the code points U+0000 to U+2FFFF (Z3 and cvc5 both stop there). Rust strings go up to U+10FFFF. So a proof about strings covers the strings whose characters are all at most U+2FFFF. A string literal with a later character (planes 3 to 16: rare CJK, tags, private use) is an unknown. The cross-check, which runs proven functions on generated inputs, is the backstop.

**Helpers in contracts.** A contract can use an ordinary function, such as `type Digit = Int where small(it)` with `fn small(n: Int) -> Bool { 0 <= n && n < 10 }`. Inside a contract, a helper that terminates and cannot fail is *seen through*: its body is used, not just its contract. So `fn zero() -> Digit { 0 }` is proven and `fn ten() -> Digit { 10 }` is refuted. A helper that may not terminate is never seen through. Unfolding one would let the solver prove false things. A helper marked `terminates` is seen through, and the proof says it relied on that.

**Termination.** `verify` also says whether each function always finishes:

- **terminates**, when shown automatically:
  - no recursion, and only `for` loops;
  - a `while a < b` loop (also `<=`, `>`, `>=`, `!=`) whose bound `b - a` the solver proves goes down on every pass, unless the pass ends the loop;
  - recursion where some argument is a smaller piece of a parameter, taken apart by a `match` (`[_, ..rest] => 1 + length(rest)`);
  - recursion with a *measure*: an integer expression of the parameters that the solver proves is never negative at each recursive call, and smaller for the call's arguments. The candidates are tried automatically:
    - each integer parameter (`fact(n - 1)`, `gcd(b, a % b)`);
    - the length of each list or string parameter (`total(drop(xs, 1))`);
    - for each comparison in the body, the distance between its sides (`100 - n` in `up(n + 1)` under `n <= 100`, `hi - lo` in a binary search);
    - pairs of these compared in order, like words in a dictionary (`(m, n)` for Ackermann's function).

  It also needs everything the function calls to terminate.
- **may not terminate**, with the reason: a `loop`, other recursion, mutual recursion, or a call to such a function. The viewer marks these.
- **assumed**: the function is marked `terminates` (after its contract, `fn f(x: Int) -> Int terminates { … }`). Its callers say they terminate *if* it does.

**Policies.** A package's `strata.toml` says what `verify` requires, each rule as a list of modules (`"*"` for all of them, `"main"` for the root module; a module covers the modules below it):

```toml
[verify]
proven = ["core"]        # every check proven, not just fuzzed
terminates = ["*"]       # every function shown (or marked) to terminate
no_recursion = ["core"]  # no recursion at all, even recursion that terminates
```

Recursion is allowed unless `no_recursion` says otherwise. A run that breaks a rule fails, listing each violation. `proven` accepts assumed code, and the run lists everything it accepted that way. `verify --require proven,terminates,no-recursion` applies the same rules to everything checked, package or not. `strata ci` rejects a rule that names a module the package doesn't have.

**Cross-check.** A proven function that terminates is still run on a few generated inputs (`--cross-check N`, 20 by default, 0 turns it off). If one of them fails, the prover was wrong, and `verify` says so: the result is a failure *found by the cross-check*, flagged as a bug in the prover. Each run has a step budget. A run that goes past it, or past the recursion limit, proves nothing either way and is set aside: proven code can still be far too costly to run, like Ackermann's function.

**Solvers.** `z3` is found on the `PATH`. `STRATA_SOLVER` names another solver binary, for example `STRATA_SOLVER=cvc5`. A solver whose `--version` names cvc5 is run with cvc5's flags; any other is run like Z3. Both get the same SMT-LIB queries. Without a solver, everything is fuzzed. Each query has a deterministic resource limit rather than a timeout (`rlimit` for Z3, `--rlimit-per` for cvc5, with a 20-second time limit only as a backstop), so results can be cached by content hash. The solver's version is in the cache key. The test suite passes with either solver. cvc5 has no Homebrew formula; install it with `brew tap cvc5/cvc5 && brew install cvc5`, or from its releases.
