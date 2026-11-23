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

## Reference

`strata verify` turns every contract into proof obligations and gives each function one of these results:

- **proven** (`✓`): the SMT solver proved every check. `--checks` lists them.
- **fuzzed** (`~`): some checks could not be proven, so the function was run on generated inputs and nothing failed. Each unproven check says why, for example *depends on `fold` is not modelled*.
- **failed** (`✗`): a counterexample. When the solver finds one, it is run (compiled) first, and reported only if it really fails. Otherwise the fuzzer looks for one.
- **incomplete** (`○`): the function still has a hole.
- **assumed** (`≈`): trusted, not checked. These are a capability's own code (`capability ...`: std's `Terminal`, `Disk` and `Process`, and a program's own) and C reached through `extern c`: they call the system. Their contracts are taken as true where they are called, and the code around them is checked as usual: a function that calls C, or takes a capability, is proven from those contracts (compiled code still checks a C function's `ensures` at each call, as nothing proves it). Only running it is different: code taking a capability isn't fuzzed (only `strata run` gives one), and what the solver doesn't prove of it is reported as skipped, saying why.

Derived code is checked as well: `derive Json for Signup` makes `Signup.to_json` and the rest, and also copies of `std.json`'s helpers made for `Signup`'s fields (`std.json.encode_as.for.Signup.4`), which are listed and checked with it, so a fault that only some field types can cause is reported there (with `integers = "proven"` under `[json]`, an `Int` field that may be beyond what JSON holds exactly).

The checks are exactly the ones compiled code makes when it runs: a callee's parameter refinements and `requires`, the refinements of constructed values and assigned fields, annotated bindings, `assert`, division by zero, and the function's own result type and `ensures`. Calls are checked against the callee's contract, never its body, which is how modules meet. A function passed by name (`lines.map(line_total)`) is checked against one arbitrary element of the list.

How it works (`src/smt.rs`): the body is run symbolically, with values as SMT terms. Records, sums, tuples and unions are SMT datatypes (`declare-datatypes`, `src/smt/data.rs`): a parameter's outer level is its fields, and a part that is itself structured is one term of its type's datatype, opened through the datatype's selectors the first time a field is read or a pattern looks inside. So a value of a large recursive type (a syntax tree) costs what the function reads of it, not the size of its type, and the solver knows by itself what datatypes give: equal values have equal parts, two constructors are different values, and a value is never one of its own parts (`t != t.left` is proven). What a type's refinements say of a term is a predicate of the term, defined for each term the code opens; a list inside a datatype value is a term of a list sort with its length and items as functions of it. A list is known by its length and by how its elements are made (a checked type, a literal, `range`, `filter`, `map`, `+`, `push`, a part of another list), position by position where it can be, so `all(xs, p)` is proven on an arbitrary element and, once known, gives `p` to every element the code looks at, a value of a union (`Square | Circle`) is a tag saying which member's shape it has plus a value for each member (a member value passed where the union is expected, or branches returning different members, become one; a `match` arm narrows it to its member), branches are merged with `ite`, and a loop keeps of the variables it changes only their declared types and its invariants, inferred or written (after a `while`, also that its condition is false, unless a `break` left it). Each check becomes the question "can the negation be true under what is known here?", which is sent to the solver (Z3, or cvc5) as SMT-LIB text (`verify --smt` prints the queries, termination ones included; `verify --obligations` prints each obligation with the solver's answer, `proven`, `refuted` or `unknown`, nothing run; `verify --termination` prints each function's termination verdict and whether a contract may see through it; `verify --fuzz` prints the fuzzer's verdict alone, no solver). `unsat` means proven; `sat` comes with a model, which becomes the counterexample. Anything not modelled (floats, dicts, sets, positions after `filter` and `sort`, most higher-order builtins, unions whose members can share a shape such as `List[Int] | List[String]` or include a type parameter) becomes an unknown value. That can only make a proof fail, never make a false proof succeed, and the reason is kept for the report. `Int` is a mathematical integer, as it is when the code runs.
An SMT string is a sequence of code points, so `len(s.code_points())` is its length exactly. A string's positions are byte offsets: `byte_len(s)` is a function of the string known to lie between its number of code points and four times that, and equal to it on an ASCII string; `byte_slice`, `index_of` and a match's position are exact on ASCII strings and bounded elsewhere. So refinements on lengths and positions stay provable. These string builtins are modelled exactly: `+`, `contains`, `starts_with`, `ends_with`, `chars` (its length, each element one character), `replace` with a pattern that isn't empty (an empty one puts the replacement around every character, which SMT's `replace_all` doesn't, so it is unknown), `to_string` of an `Int`, `Bool` or `String`, and `is_digit`. `parse_int` is exact for digits with an optional `-`, and for strings without a digit (`None`). Other forms (whitespace, `+`, `_`) are unknown. `trim` keeps only a length bound. `to_upper`, `to_lower`, `is_alpha`, `split`, `lines`, `repeat` and `reverse` are unknown. `chr` is a check that its argument is a character, and `ord` one that its argument is one character. `matches(s, "regex")` becomes a regular-expression constraint with `std.regex`'s meaning (the prover's model is made by `std.regex.smt_model`). Strata regexes are Unicode regexes: `\d` is any decimal digit, `\w` and `\s` are Unicode classes, `\p{..}` works, `.` is any character but `\n`, and `(?i)` is Unicode case folding. The pattern is parsed by `std.regex`, and translated exactly: literals, classes, groups, alternation, repetition, and `^` / `$` at the very start or end. Word boundaries, multi-line anchors and anchors elsewhere stay unknown. Z3 can't decide classes with hundreds of ranges (`\w`, `\p{L}`, Unicode `\d`), so such a class is bounded from both sides: `matches` is true on a subset of its ranges and false outside a superset of them. When one string is matched against two patterns, how their languages relate (one inside the other, or disjoint) is worked out exactly from the patterns' automata and added as a fact. So `\d{3}` implies `\d+`, and `[a-z]+` implies `\w+`.

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

`verify NAME` analyses termination only for what the selected functions reach (a function's verdict depends only on what it calls), so checking one function does not pay for the loops of the whole program and std.

**Policies.** A package's `strata.toml` says what `verify` requires, each rule as a list of modules (`"*"` for all of them, `"main"` for the root module; a module covers the modules below it):

```toml
[verify]
proven = ["core"]        # every check proven, not just fuzzed
terminates = ["*"]       # every function shown (or marked) to terminate
no_recursion = ["core"]  # no recursion at all, even recursion that terminates
```

Recursion is allowed unless `no_recursion` says otherwise. A run that breaks a rule fails, listing each violation. `proven` accepts assumed code, and the run lists everything it accepted that way. `verify --require proven,terminates,no-recursion` applies the same rules to everything checked, package or not. Loading the package fails on a rule that names a module it doesn't have.

**Cross-check.** A proven function that terminates is still run on a few generated inputs (`--cross-check N`, 20 by default, 0 turns it off). If one of them fails, the prover was wrong, and `verify` says so: the result is a failure *found by the cross-check*, flagged as a bug in the prover. Each run has a step budget. A run that goes past it, or runs out of stack, proves nothing either way and is set aside: proven code can still be far too costly to run, like Ackermann's function.

**Inputs from the solver.** Random values rarely meet some types: a list whose elements are consecutive integers, or a record whose indices must stay within its own list. When the fuzzer can't make inputs for one ("could not generate a value", or every input broke the `requires`), the solver makes them. It looks for values that satisfy the parameters' types and the `requires`, with lists of at most 6 elements. Up to 20 of them, each different from the ones before, are run as the fuzzer's cases. A failure is reported as *found by the fuzzer, on inputs from the solver*:

```text
✗ last                     Ensures violation (found by the fuzzer, on inputs from the solver)
    input:   [-1, 0, 1, 2]
```

**The solver and the fuzzer race.** A function's obligations go to the solver and its inputs to the fuzzer at the same time. The fuzzer waits up to 0.3 s first, because most proofs take milliseconds. Until the solver has answered, the fuzzer runs on a cross-check's budget of steps. The first to find a failure stops the other, and the solver's processes are killed. So a query the solver would give up on after 20 s doesn't hold up a bug the fuzzer finds in a millisecond. When the solver proves everything, the fuzzer runs only the cross-check's cases. When it doesn't, the fuzzer runs the full search, and the cases that ran out of the smaller budget are run again first. Which side finishes first depends on timing, so a failure the fuzzer finds before the solver has proven everything is reported as the fuzzer's, not as a bug in the prover. `--no-race` runs the solver first, then the fuzzer: every result is the same from run to run, and the cross-check can tell a bug in the prover.

**Verdicts are kept.** Each check's verdict is kept between runs, for a package, a script or `std/` alike: in the git repository the code is in (`<git common dir>/strata/verdicts`, so every worktree shares them), else in `~/.cache/strata/verdicts` (`STRATA_CACHE`; `STRATA_VERDICTS` names the folder). Each verdict is a file of its own, written whole and renamed into place, so several runs at once share them safely. Its key is the check's name, the hash of its definition with everything it depends on (`store::closure_hash`), the number of cases and of cross-checked cases, the seed, `--no-race`, the solver (path and version) and std built into strata (unless std itself is checked: then its definitions are in the hashes) (not the strata binary: a verdict outlives rebuilds of the compiler). Only the definitions with a check to run get fuzz drivers compiled. Each definition's latest verdict is kept too, by its hash: what `strata ui` and `strata diff` show. A changed definition is checked again with what depends on it, and nothing else is: a kept verdict is printed as a fresh one with `(cached)` after it, and `--require` and `[verify]` apply to it the same way. `--no-cache` checks everything again (and keeps the new verdicts). A verdict where the solver gave up isn't kept: its time limit may have run out on a busy machine. Folders of the older layout, one per compiler, unused for two weeks are removed.

**Std's termination is kept.** What the solver finds about std's own functions (a loop's bound, a recursion's measure) is kept between runs in `~/.cache/strata/termination` (`STRATA_CACHE` moves it), one file per strata binary and solver: std is part of the binary, so it can't change in between. Functions made from std for a program (`std.json.encode_as.for.Signup.4`) are the program's, and are analysed every time.

**In parallel.** Functions are checked on all cores (at most one solver process per core), and a function's obligations are solved in one solver process, each between `push` and `pop` (starting Z3 takes longer than most queries). Termination's loop bounds and recursion measures are found the same way. No string is longer than memory: each query bounds the lengths it uses below 2^64, without which Z3 tries to build a string of 10^308 characters to refute `len(s) < 10^308` (it gave up after 20 s on `float(len(w))`).

**Solvers.** `z3` is found on the `PATH`. `STRATA_SOLVER` names another solver binary, for example `STRATA_SOLVER=cvc5`. A solver whose `--version` names cvc5 is run with cvc5's flags; any other is run like Z3. Both get the same SMT-LIB queries. Without a solver, everything is fuzzed. Each query has a deterministic resource limit rather than a timeout (`rlimit` for Z3, `--rlimit-per` for cvc5, with a 20-second time limit only as a backstop), so results can be cached by content hash. A proof gets 2M Z3 units, a tenth of what a search for a model gets (inferring loop invariants, making inputs): every proof in std, the examples and the compiler takes under 0.5M, and what ran longer was a search for a counterexample to a check that can't be proven (it is "the solver gave up" now; the fuzzer looks for one too). The solver's version is in the cache key. The test suite passes with either solver. cvc5 has no Homebrew formula; install it with `brew tap cvc5/cvc5 && brew install cvc5`, or from its releases.
