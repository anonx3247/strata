# Rebuilding Strata

The plan for a Strata rebuilt in small layers, each read and understood before the next, with the lab (this repository's current compiler, core and std) as its template and testing ground. ISSUES.md lists the design problems the rebuild solves; BUGS.md the lab's bugs; IDEAS.md possible future changes.

## Where it lives

- In this repository, in its own folder (`next/`, name to settle), beside the lab. Releases stay the lab's until the rebuild can replace it; then the lab's folders are deleted in one PR, and history keeps them.
- Each PR touches the lab or `next/`, never both. `strata upgrade` and `tools/check` are scoped to one of them.
- The rebuild's code is edited with the lab's `strata` until the rebuild compiles itself (end of L2).

## Rules

1. **Same language, smaller.** The rebuild implements a subset of the lab's syntax and meaning. A difference from the lab is a decision written here.
2. **Written in the layers before.** The compiler's own code uses only features of earlier layers, so it compiles itself at the end of L2. Sugar doesn't count: its stored form is the core.
3. **One layer at a time, each complete**: its tests (proofs from L1 on) pass and the user has read its tours.
4. **One module per concern.** A new command or feature is a new module.
5. **Small PRs, each with a tour** (`strata tour`, `.tours/BRANCH.md`).
6. **Simple before fast.** No optimization before the layer for it.
7. **The stored tree is the core language.** Sugar exists only in the parser (sugar to core) and the printer (core to sugar), one builder/reader pair per form, tied by the law `parse(print(t)) == t`. Operators are trait methods: `a + b` is stored as `Add.add(a, b)`.
8. **One way to call C.** The runtime is reached only through `extern c` declarations in core, each with `assumed` contracts: the trusted base under every proof.
9. **Lab first.** Anything the lab doesn't have is tried there before the rebuild builds it.
10. **No catch-all yes.** A pass that doesn't know answers "unknown" or reports an error where the unknown arises. No `TAny`, no fuel that answers true, no `_ => true`, no limit that answers "exhaustive" or "finite". (ISSUES 2)
11. **Facts as code, not comments.** A property of the code is written as a contract, invariant, law or lemma, where it is checked. Comments never claim what code does. Doc comments: one line or a few for each module, type and function; a comment elsewhere only to name or sketch an algorithm. A lint enforces it.
12. **No names in the compiler.** Built-in types and functions are declared once in core; the compiler finds them by id, never by spelling. (ISSUES 4)

## Decisions in force

- **Storage**: a module file is its syntax tree in the binary format (header first, interface before bodies, self-contained definitions, canonical). References by stable id: every definition, variant, method and impl has a 32-bit id per module, kept through edits, renames and moves; the header maps ids to names. Hashes are of the named encoding. No `strata.log`: git is the history, one commit per change by default.
- **Values**: immutable by default; `mut` variables and `mut` parameters (passed in place, never aliased); no shared mutable references, no cycles (a language invariant: counting frees everything only because no value reaches itself).
- **Errors are values**: `Result`, `Option`, `!`, `?`, `except`, `or`; no error conversion; a panic ends the program and can't be caught.
- **Numbers**: sized integers `I8`..`U64` from L0, overflow a panic proven absent where possible (explicit wrapping methods); arbitrary-size `Int` the default for programs at L4, the compiler's own code on sized integers; decimal literals are exact `Real` unless a `Float` is wanted; f-strings interpolate, no format specs.
- **Types**: records, sums (unions of variants or of types), generics with trait bounds, structural record bounds, refinement types, contracts. One impl per trait and type, with an orphan rule. Dicts are hash maps with no order.
- **Effects**: capabilities are ordinary traits passed as parameters; no effect system, no monads. Purity is inferred (no capability, no `extern`, no `spawn`) and shown.
- **Verification**: SMT first (Z3, cvc5 raced), written proofs (`lemma`, `assert … by`, `calc`), no dependent types or tactic language.
- **Concurrency**: structured, uncolored tasks (`scope`, `spawn`, `await`), channels with explicit `close`, `select`, `race`, supervised tasks. No object is ever counted by two threads: no atomic counts.
- **Memory**: reference counting, with scoped allocation from roots layered on it where proven (designed and measured in the lab first).
- **std** lives in this repository and is its own work stream.

## Solutions to the issues

| Issue (ISSUES.md) | Solution | Layer |
|---|---|---|
| 1. Facts by name and text | Node ids: (definition id, postorder index). Every pass returns side tables keyed by node id: types, verdicts, roots. Checks are removed by node id. | L0 |
| 2. Default to yes | Rule 10. Unification with an occurs check; bidirectional checking (expected types down, inferred types up); exhaustiveness in one place, on every `match`. | L0 |
| 3. Walks that drift | One traversal interface per tree type (named slots, rebuild from children), generated; folds and rewrites over it written once; dependencies computed through it. | L0 (written by the lab's `strata`), generated at L6 |
| 4. String-keyed tables | Stable ids for definitions, variants, methods, impls; rule 12. | L0 |
| 5. Modules with many jobs | A small mid-level IR between the typed tree and LLVM: blocks, typed values, explicit `dup`/`drop`. Lowering, counting and memory passes are separate modules over it. | L0, counts at L2 |
| 6. Terms as strings | SMT terms as a hash-consed datatype, printed once. | L1 |
| 7. Termination | A body is used in a proof only once its termination is proven: structural recursion on sums first, then inferred measures, then `decreases`. One transparency decision. | L1 |
| 8. Three verdict paths | One function from solver answers to verdicts. | L1 |
| 9. Cache keys | Proofs keyed by the definition's interface and its callees' interfaces; a body only when it is seen through. Headers checked on read. | L1 |
| 10. Specs not checked | Vacuity checks: a `requires` that never holds, an `ensures` implied by types alone. Spec changes shown apart in tours. | L1 |
| 11. One thread per task | A green-task scheduler, single-threaded first; every wait through one park interface; a finished task freed when its handle is dropped. | L3 |
| 12. One global flag | The runtime's invariants as contracts on core.runtime; one owner per allocator. | L0, L3 |
| 13. A word per field | Fields and elements at their own size; `Array[T]` with records inline; `Option` of a pointer as a nullable pointer; flat trees with interned names. | L4, L7 |
| 14. Invisible copies | Frozen values (immutable, never counted) readable by any task; copies reported per `spawn` and `send`. | L3 |
| 15. Strings trust bytes | One boundary from bytes to text returning a `Result`; a byte buffer type. | L0 |
| 16. Write before check | Every command that changes code: merge in memory, check, write the change atomically. | L5 |
| 17. Edits reach outside | An edit touches only its definitions; dependencies fetched on request only; merges on trees. | L5 |
| 18–20. std | The std work stream, ordered by the layers each module needs. | std |

## Layers

Each layer lists what it adds; nothing else exists yet.

### L0: a compiler that runs programs

- Types: sized integers, `Bool`, `String` (UTF-8 bytes, checked at the boundary), `List[T]`, records, sums, `Option`, `Result`, generics without bounds.
- Code: `fn`, `:=`, `mut`, `if`, `match` (exhaustive), `while`, `for x in list`, `return`, `break`, `continue`, calls, string interpolation. Operators as calls of core's operations on built-in types.
- Modules, `use`, `pub`, a package with `strata.toml`, path dependencies.
- Capabilities `Console`, `Files`, `Process` as concrete types; `extern c` with contracts.
- The compiler: lex, parse (with the sugar pairs that exist), resolve (ids), check (bidirectional, side tables), print, binary read and write, the mid-level IR, LLVM IR text, link with `clang`.
- Node ids and the generated traversal interface (generated by the lab's `strata` until L6).
- Runtime: one region for the whole run, no counting, about 550 lines of C.
- Commands: `view`, `edit`, `check`, `build`, `run`.

### L1: verification

- Refinement types, `requires`, `ensures`, `old`, named results; the implicit checks (overflow, division by zero, index range, exhaustiveness) as obligations.
- `verify` through Z3 and cvc5, on the core tree, terms hash-consed. Cheap obligations (linear, no quantifiers) checked during `check` and `edit`; the rest in `verify`, same encoder.
- Termination before transparency (Issue 7). Loop invariants derived, then written.
- Written proofs: `lemma`, `assert … by`, `calc`; deterministic steps `reveal f`, `by compute`, `by nonlinear`, `by bits` (every `*` on sized integers is a nonlinear obligation).
- Proven checks removed by node id; the rest stay run-time checks.
- Vacuity checks; proof stability checks (shuffled and renamed queries) on core and std.
- Evidence beside the tree, keyed by hash: verdict, solver version, resource use, unsat core (facts used), so a re-proof sends those first.
- Fuzzing for what the solver can't decide, reported as fuzzed, never as proven.

### L2: enough to compile itself

- Traits, impls, methods, bounds; one impl per trait and type, orphan rule; generics verified once against their bounds, compiled per type.
- `Console`, `Files`, `Process` as traits; `mock`.
- `Dict`, `Set` as hash tables; lambdas and closures; `Display`/`Debug`.
- Reference counting on the mid-level IR, parameters borrowed by default (inferred per function).
- Milestone: the compiler builds itself, fixed-point bootstrap.

### L3: tasks

- `scope`, `spawn`, `await` on a single-threaded green scheduler; every wait and I/O wait through one park interface; a finished task freed when its handle is dropped.
- Channels (a lock per channel), `select`, `race` and cancellation, supervised tasks; deadlines that nested calls inherit; a CPU loop cancellable at its back edge.
- Values crossing tasks are copied, or frozen and shared; copies reported.
- `Clock`, `VirtualClock`; a seeded scheduler for `strata test`, std mocks of network and files that inject faults.
- Pinning a task to one thread, for C libraries with thread state.

### L4: numbers and layout

- `Int` (arbitrary size), `Real` (exact rationals), `Float` (opaque in the solver, interval facts).
- Fields and list elements at their own size; `Array[T]` with records inline; `Option` of a pointer as a nullable pointer.

### L5: tools

- `diff` and `merge-file` on trees, node by node; `find`, `refs`, `rename`, `move`, holes and pending changes; `--json` on every command.
- One write path for every command that changes code (Issue 16).
- Semantic versioning computed from interfaces and contracts: a stronger `requires` or a weaker `ensures` is a breaking change.
- `doc`, `docgen`, `skill`, `tour`; an LSP later.

### L6: compile time

- `comptime`, `derive` (derivable traits in Strata, each derive proven once), `std.typing` with trees, not source text.
- The traversal interface generated by `derive Tree`.

### L7: speed and memory

- The memory model as settled in the lab: scoped allocation from roots, loop bodies as regions, static uniqueness, in-place updates, small values in registers, borrowing from the parent scope across tasks.
- Flat trees with interned names; LLVM, lld and Z3 in process; content-addressed caches; parallel passes; more cores for the scheduler.

## Work streams beside the layers

- **Sugar**: each form a parser rule and a printer rule with the round-trip law. Handlers (`!`, `?`, `except`, `or`, `!!`, `??`) first, then defaults and `..r` updates, ranges and indexing, comprehensions, `loop`, spreads.
- **std**: io, fs, process with L0; collections, json, toml after L2; regex (search, captures, replace, compiled once) after L2; http with supervised connections, deadlines and a connection cap after L3; web with the Elm architecture's commands after L3; byte buffers, paths, dates, terminal input; Linux releases.

## Before L0: the spec, then phase 0

**The spec comes first** (user, 2026-11-27): one document, as short as it can be while explicit about everything, that a person can read end to end and an agent can build the language from. Written section by section with the user, working through problems as they come:

1. Lexical structure: tokens, literals, comments, layout.
2. Syntax: the core grammar, and every sugar with exactly the core form it is stored as.
3. The tree: every node, its fields, how definitions are stored.
4. The binary format and hashing: files, headers, ids, the three hashes.
5. Names and modules: resolution, imports, visibility, packages.
6. Types: the type language, checking rules, generics, traits and coherence, unions and narrowing, refinements.
7. Semantics: evaluation, values, mutation, errors and panics.
8. Verification: obligations, contracts, termination, written proofs, what "proven" means.
9. Concurrency: tasks, channels, `select`, `race`, cancellation, supervision.
10. Memory: values in memory, counting, regions, what crosses tasks.
11. The runtime interface and `extern c`.
12. The core library: built-in types and operations, with contracts.

A test of the spec: an agent (Fable) builds the language from it in one go, with the lab's `strata`. How far it gets says what the spec leaves out.

Phase 0, done: `view` in reading order (#547), fast `diff` with `--json` (#544), PR tours (#546, #550). Left:
- The lab bugs that lose code or prove false facts (BUGS.md, first entries).
- Format trials in the lab: stable ids (#559) and the stored core (#562), merged and released.
- The rules 10 and 11 tried in the lab: a lint for claim-style comments; the remaining `TAny` and default-yes answers replaced by errors in one module, measured.

## Appendix: decisions recorded in the lab, to review

The decisions the old IDEAS.md recorded as in force (origin/main 1799da47). The ones above take precedence; each of these is kept, changed or dropped as the spec is written.


## Values, errors and mutation
- Errors are values (`Result`, `Option`); there are no exceptions. A panic can't be caught and ends the whole program.
- An unproven contract or refinement stays a run-time check; a proven one leaves the compiled code.
- Partial operations (`xs[i]`, `/`, `sqrt`, `ord`) carry preconditions that `verify` discharges at each use.
- No error conversion: `!` passes an error on only when it fits the function's error type (same type, a variant into its sum, a member into a union).
- Values are immutable by default; mutation is declared (`mut` variables, `mut` parameters, `mut self`) and written back to the caller's variable.
- `mut` is marked at the call site: `f(mut x)`, `mut q.claim(2)`; results are not places. `mut` arguments come after the other arguments.
- References are second-class: a mutable reference exists only as a `mut` parameter, never stored or returned; no borrow checker.
- A `mut` argument is passed by reference and may not overlap another argument (decided, not built).
- A field may be a `mut` argument (`shift(mut r.lo)`), checked as the equivalent assignment.
- Collections change through `mut` methods (`mut xs.push(x)`); new values are expressions (`xs + [x]`, `xs.sorted()`); one way to say each thing.
- `d.keys()` stays a `List`; `d.key_set()` is the set.
- Bindings and assignment as decided 2026-11-21 (`x := e`, `mut x := e`, `x = e`).
- Mutation is in place whenever nothing else holds the value (Perceus-style counting with reuse).

## Types
- Sums and unions are one thing; variants are types; `match` patterns test them.
- Refinements are structural; `type X = Y` with no `where` is an error; `distinct type` is nominal.
- Dicts and sets are unordered (hash tables); iteration order is promised nowhere; hashing is deterministic.
- No explicit type arguments at calls: inferred from the arguments and the expected type. A `_` left in type arguments is not an error.
- Integers are arbitrary precision; sized integers (`I8`..`U64`) wrap; `/` and `%` are Euclidean; `Float` is never NaN or infinite.
- Exact rationals (`Real`) for decimal arithmetic where asked for; `Float` a type of its own.
- Strings are not iterable or indexable; units are named (`bytes`, `code_points`, `graphemes`); positions are byte offsets; `byte_slice` is total.
- Ranges: `a..b` excludes `b`, `a..=b` includes it; slices use ranges; no negative indices; `xs[i]` requires the index in range, `get` gives an `Option`.
- Special methods `@range`, `@slice`, `@step` and `by` steps (decided 2026-11-21, not built); `map`/`filter`/`fold` are ordinary iterator methods.
- `??` / `or {}` only on `Option`, `!!` / `except {}` only on `Result`.
- `Iterator` and `FiniteIterator` (a supertrait); `map`/`filter` lazy; collections give iterators through `xs.iter()`.
- `Debug` (the value as code) and `Display` (readable form) are derived traits every type has, overridable.
- Record field defaults: any expression with no variables, evaluated at each use.
- Structural records `{ f: T, .. }` as a parameter type or bound; closed anonymous records `{ f: v }`; nominal records stay nominal; compiled by monomorphization.
- Spread `f(...x)` fills parameters by name, positional arguments win; decided by the checker.
- Proven `ensures` narrow a call's result type; the callee's proof is not part of its interface hash.
- `static` values: decided against.

## Contracts and verification
- Contracts relate values; refinements say what one value is (a one-parameter `requires` is that parameter's refinement: warnings fixed by `strata fix`).
- Named results need two or more elements.
- `old e` in `ensures`; trait models, trait-level `invariant` and `ensures`; code never reads a model.
- `assumed` contracts are trusted: proven or refuted by `verify`, never checked at run time unless asked.
- `shared` traits: each method is one atomic step; one write to outside state per path.
- Laws are checked per impl and are not facts for callers.
- No written `flow` blocks: state graphs are generated.
- `verify` and `build` never search for loop invariants; the search runs on `edit` and `fix` and writes what it finds; a `for` gets its `ensures` over the elements visited.
- Regex refinements are proven over ASCII by default and say so; full Unicode on request (decided 2026-11-20, not built).
- Proof budget stays at 2M rlimit units; a query that ran out of time is told from one that gave up by the units used.
- `verify` never gives a real capability and never runs code reaching an `extern`.
- Optimization only with `--release`.

## Capabilities, effects and concurrency
- No effects: capabilities are traits passed as parameters or held in fields; `mock T` implements a trait from its contracts.
- A `capability` can't be mocked, only made by the runtime; its traits can. `mock` declares a test-only type.
- Concurrency: structured tasks without coloring (`scope`, `spawn`, `await`, channels, `select`, `race`, supervised tasks, a `Clock`); no `async` functions.
- Tasks get copies; no atomic counts.
- A REPL line gets the REPL's own capabilities unless `--allow` says otherwise.

## Memory
- Reference counting with reuse; no cycle collector (immutable values make no cycles); no manual memory, no unsafe mode; the escape hatch is `extern c`.
- Next steps remove counting where the compiler sees it isn't needed, chosen by measurement.

## Modules, packages and storage
- A package is a folder with a `strata.toml`; a file is a module; module name is its path in the package; `core` and `std` names are reserved.
- Imports are Python-like: `use std.time`, `use std.time as t`, `from std.time use Clock`.
- Modules meet only through `pub` contracts.
- Code is stored as a binary tree (`.strata`); text only through edits; references stored qualified, shown short; format version stays 1 until settled; no backward compatibility.
- Definition hashes are SHA-256, computed when stored, trusted on read.
- Markup is stored as calls to `std.web`; escaping by construction; verifying markup is not a goal.
- Output is deterministic (the bootstrap needs the same bytes twice).

## Tools
- Git stays the base; each change is a commit; `.strata/` is a local cache; no CI.
- The cache is cleaned by hand, never evicted.
- A failed edit writes nothing; `--as-given` writes anyway.
- A merge conflict inside a definition is a hole `?conflict_N(ours, base, theirs)`; merges work on stored forms.
- `view` shows definitions in reading order (imports, types, functions, tests; depth first).
- `revert` overwrites; `history` shows diffs by default.
- Run first, verify alongside; never compile as if proofs will succeed.
- Live edits swap bodies only; an interface change is refused.
- Rendered code wraps at about 80 columns; the viewer has two levels and no Flow view.
- `strata doc` sections are written `PAGE#SECTION` only.
