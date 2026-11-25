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

`factorial` is proven for every input: its result is always positive, and it always terminates. `verify` refutes `next_digit` before anything runs, and shows the input that breaks it.

## What Strata is for

- Types state constraints. Strata has refinement types (`Int where 1 <= it && it <= 99`), `requires` and `ensures` contracts, algebraic data types, unions and exhaustive matching.
- Contracts are proven. An SMT solver (Z3 or cvc5) proves what it can, and a fuzzer tests the rest. Each result says which of the two it came from. Termination is proven too, or reported as unproven.
- You edit definitions. Each definition is stored as a syntax tree and identified by the hash of its content. Text is a view of it, at the level of detail you ask for. Every edit is checked against the whole codebase before it is committed.
- Incomplete programs still type-check. A typed hole (`?name`) has a goal type, a context and obligations, which together make a precise task to hand to an agent.
- Capabilities are values. A database or a clock is a trait passed as a parameter. A function's signature shows what it can touch, and a test passes a `mock`.
- Values print as code. Messages, traces and counterexamples show each value the way it would be written (`Circle { radius: 2 }`), and `strata run f ARGS` reads its arguments in the same syntax. JSON is a library (`derive Json`) that checks what it reads against the types, refinements included.
- Concurrency has no function colouring. `scope`, `spawn` and `await` run tasks on threads. Tasks communicate with channels, `select` and `race`, which cancels the losers. Any function can block, and there is no `async`.

> Status: **an experimental draft**. The compiler is written in Strata (`compiler/`) and builds itself. It lexes, parses, type-checks, verifies (with Z3) and generates code through LLVM, either to an executable (`strata build`) or in memory. The in-memory JIT runs `strata run`, the fuzzing in `verify`, and compile-time code. The runtime is in C (`runtime/runtime.c`), except for the memory allocator, which is written in Strata (`core.rt`).

## Try it

On macOS (Apple silicon), install the latest release into `~/.strata`. The release includes LLVM, the runtime, std and Z3, so nothing else is needed.

```sh
curl -fsSL https://strata-lang.dev/install.sh | sh
```

Set `STRATA_VERSION=v0.1` to pin a release, `STRATA_INSTALL=DIR` to install somewhere else, or `STRATA_MODIFY_PATH=1` to add its `bin` to your shell's PATH. Running the script again upgrades.

To build from source, you need an existing `strata`, because the compiler is written in Strata. `tools/bootstrap.sh` uses `STRATA_SEED` if set, then a `strata` on the PATH, and otherwise downloads the latest release. The build links LLVM 23 and lld into the binary, so they are needed to build it but not to run it.

```sh
brew install llvm zstd                 # LLVM 23, and clang
tools/build-lld.sh                     # lld's libraries, into target/lld
tools/bootstrap.sh                     # target/strata, built by itself (a fixed point)
alias strata=$PWD/target/strata

# Code is stored as its syntax tree (binary .strata files): read it as text…
strata --file examples/orders.strata view main

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

- [The language](docs/language.md): types, refinements, unions, traits, modules, mutation, errors, tasks and programs.
- [Verification](docs/verification.md): what `strata verify` proves, with examples and the verifier's output.
- [Tools](docs/tools.md): the command line, structural diffs, storage, packages and git.
- [Learn Strata in Y minutes](docs/learn-strata.md): the whole language in one valid file, written for agents.
- References: [the language](docs/language-reference.md), [verification](docs/verification-reference.md) and [the tools](docs/tools-reference.md).
- [Internals](docs/internals.md): the source layout, `core` and the compiler. [The binary format](docs/binary-format.md) describes `.strata` files.
- [IDEAS.md](IDEAS.md): decisions, open questions and plans.
- [site/](site/README.md): the website, strata-lang.dev, with a landing page and a tour built from the real tools' output. [docs-site/](docs-site/README.md) publishes these docs there.
