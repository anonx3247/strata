# Strata

An experimental programming language designed for working with agents: code that says what it means, checked and proven as it is written.

```rust
/// A decimal digit: an `Int` with a constraint, checked wherever one is made.
type Digit = Int where 0 <= it && it <= 9

/// n! is never 0.
fn factorial(n: Natural) -> Positive {
  if n == 0 { 1 } else { n * factorial(n - 1) }
}

/// Wrong: a digit plus one isn't always a digit.
fn next_digit(d: Digit) -> Digit {
  d + 1
}
```

```text
$ strata --file examples/math.strata verify
✓ factorial                proven (2 checks)
✗ next_digit               Refinement violation (found by the solver)
    input:   9
    result of `next_digit`: 10 does not satisfy `Digit` (where 0 <= it && it <= 9) (stack: next_digit)
```

`factorial` is proven for every input: its result is always positive, and it always terminates. `next_digit` is refuted with the input that breaks it, before anything runs.

## What Strata is for

- **Constraints live in the types.** Refinement types (`Int where 1 <= it && it <= 99`), `requires` / `ensures` contracts, algebraic data types, unions and exhaustive matching.
- **Contracts are proven, not just tested.** An SMT solver (Z3 or cvc5) proves what it can; what it can't prove is fuzzed, and every result says which. Termination is shown too, or flagged.
- **You work on definitions, not files.** Definitions are syntax trees identified by content hash; text is a view at the level of detail you need, and every edit is checked against the whole codebase before it is committed.
- **Incomplete programs still check.** A typed hole (`?name`) has a goal type, a context and obligations: a precise task to hand to an agent.
- **Capabilities are values.** A database or a clock is a trait passed as a parameter, so a signature shows what code can touch, and a test passes a `mock`.
- **Values read as code.** Every value is shown as it would be written (`Circle { radius: 2 }`), in messages, traces and counterexamples, and `strata run f ARGS` takes its arguments that way. JSON is a library (`derive Json`), read against the types, refinements included.
- **Concurrency without colouring.** `scope`, `spawn` and `await` run tasks on threads, with channels, `select` and `race` (the losers cancelled); any function can block, and there is no `async`.

> Status: **an experimental draft**. The compiler is written in Strata (`compiler/`) and builds itself: lexing, parsing, checking, verifying (with z3), and generating code through LLVM, to an executable (`strata build`) or in memory (the JIT, for `strata run`, `verify`'s fuzzing and compile-time code). The runtime is C (`runtime/runtime.c`) with its memory allocator written in Strata (`core.rt`).

## Try it

Install a release (macOS on Apple silicon; nothing else to install):

```sh
curl -fsSL https://strata-lang.dev/install.sh | sh
```

Or build it from source: Strata is written in Strata, so a `strata` builds the next one (`tools/bootstrap.sh` takes `STRATA_SEED`, else a `strata` on the PATH, else downloads the latest release). It links LLVM 23 and lld in, so building it needs them (only to build: the result needs nothing installed):

```sh
brew install llvm zstd                 # LLVM 23, and clang
tools/build-lld.sh                     # lld's libraries, into target/lld
tools/bootstrap.sh                     # target/strata, built by itself (a fixed point)
alias strata=$PWD/target/strata

# Code is stored as its syntax tree (binary .strata files): read it as text…
strata --file examples/orders.strata render

# …work directly on a file…
strata --file examples/orders.strata check
strata --file examples/orders.strata test
strata --file examples/orders.strata verify

# …or in a package, where every change is checked and logged
mkdir /tmp/shop && cd /tmp/shop
strata init
strata add path/to/examples/orders.strata
strata view --level 0              # map of everything
strata view slice:Order --level 1  # Order, what it uses, what uses it: interfaces only
strata holes                       # what's left to implement
```

## Documentation

- [The language](docs/language.md): types, refinements, unions, traits, modules, mutation, errors, tasks, programs.
- [Verification](docs/verification.md): what `strata verify` proves, with examples and the verifier's output.
- [Tools](docs/tools.md): the command line, structural diffs, storage, packages and git.
- Annex: [Learn Strata in Y minutes](docs/learn-strata.md) (the language in one valid file, for agents), and the references for [the language](docs/language-reference.md), [verification](docs/verification-reference.md) and [the tools](docs/tools-reference.md).
- [Internals](docs/internals.md): how the source is laid out, `core`, the compiler; [the binary format](docs/binary-format.md).
- [Ideas, decisions and open questions](IDEAS.md): where Strata is going.
- [site/](site/README.md): the website (strata-lang.dev): a landing page and an interactive tour built from the real tools' output; [docs-site/](docs-site/README.md) publishes these docs on it.
