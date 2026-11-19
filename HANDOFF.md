# Handoff

State as of the viewer UI commit (all 14 integration tests pass: `cargo test --release`).
Read `README.md` first; it describes the language and tools as they are now.

## Working conventions

- Commit each step (the user wants every change tracked) and push to `origin main` (github.com/anonx3247/strata, private).
- Keep `cargo test --release` green before every commit; check that the examples still round-trip:
  `strata --file F render > a; strata --file a render | diff - a`.
- Verify UI changes with headless Chrome screenshots:
  `"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new --disable-gpu --hide-scrollbars --window-size=1700,1000 --virtual-time-budget=4000 --screenshot=out.png "http://127.0.0.1:PORT/#level=1&sel=core.run_worker"`.
  Start `strata ui --file examples/tasks --port PORT` in the background, and kill **only that port** afterwards (`kill $(lsof -ti tcp:PORT)`); the user may have their own viewers running.
  Viewer errors show in the page banner.
  To check hover: start Chrome with `--headless=new --remote-debugging-port=9333 --user-data-dir=<scratch>`, then a small Node 22 script (built-in `WebSocket`, Chrome DevTools protocol) opens the page, sends `Input.dispatchMouseEvent` (`mouseMoved`) over an `a.ref`, and calls `Page.captureScreenshot`. Kill only that port afterwards.
- macOS: no `timeout` command (use `perl -e 'alarm N; exec @ARGV' cmd`), and BSD `sed`.

## Design decisions the user has made (do not revisit without asking)

- **No effects.** Capabilities (database, queue, console, clock) are traits; code takes an implementation as a parameter or holds it in a field. `mock T` generates an implementation that respects the trait's contracts. Async and iterators will be designed later as their own language features, **not** as effects.
- **Immutable by default, mutable when declared**: `let mut`, field assignment, `mut` parameters and `mut self` with write-back to the caller's variable. Procedural code is normal, and there are no shared references.
- **Refinements are structural**; `type X = Y` with no `where` is an error, so use `distinct type` for a nominal type.
- **Modules meet only through their public (`pub`) contracts.** A folder of `.strata` files loads as modules, and `render --split` writes them back.
- **Rendered code wraps at about 80 columns** by breaking lines, not soft-wrapping.
- **Viewer**: two levels (interface/source), the code view sliced around the selection, a type map as a separate view, and no Flow view.

## Viewer UI changes (done)

Doc comments are shown as the description in the right panel (and stripped from the code, `strip_doc` in `ui.rs`); the inspector has no source block; code items are plain code with a margin marker; bodies open inline (`has_body` in the snapshot; impls and traits now have a signatures-only level 1 in `view.rs`); the header has *Code* / *Type map* tabs and the code toolbar has *expand all*. The selected definition starts open.

Still open: **type map as a side panel** instead of a full tab. The user suggested it; ask before doing it.

## Decided with the user: git, diffs and review (not built yet)

- **Git stays the base.** Commit the `.strata` text files and the log (`log.jsonl`, append-only, entries with ids, `merge=union` in `.gitattributes`). Objects and index become a local cache rebuilt from the files. Optionally commit the verification cache.
- **Structural diff** between two git revisions: load both snapshots, pair definitions, compare trees. The log entries present at HEAD but not at the base are the atomic steps in between (for stepping through, renames, messages). A PR = merge-base..HEAD.
- **Change kinds**: added/removed; renamed/moved (one line, "N references updated", folded); signature change marked in place, with contract stronger/weaker; body change shown in place with unchanged statements folded, **falling back to a line diff of the canonical text when most of the body changed**; type field/variant changes (with "N matches must handle it"); trait changes; doc comments as a word diff; verification before → after. Ordered types → interfaces → implementations → tests.
- **`strata rename old new`**: done (`resolve::rename`). It reuses the resolver's scoping, so locals shadow and `use` imports work as the checker sees them, and it refuses a rename that would change what any other name refers to. Functions, types and traits only: variants, fields and trait methods can't be renamed yet, and docs and comments aren't rewritten (the command lists the ones that mention the old name). The log entry has `"rename": {"from", "to", "references"}`. Database only for now (like `edit`).
- **Merge driver**: `strata merge-file %O %A %B %P` per definition, then type-check. GitHub won't run it, so: `strata sync main` merges main into the branch locally, branch protection requires branches to be up to date, and a required CI check runs `strata check` + canonical-format check on GitHub's merge ref.
- **jj**: compatible (runs on git) but not a dependency.
- The tour is later. The user wants to settle the look first: offer a static mockup of the *Changes* view.

Order: ~~`strata rename`~~ → diff core in Rust → `strata diff <rev>..<rev>` → *Changes* view in the viewer → merge driver.

## Open topics the user wants to discuss and decide on (do not implement without agreement)

- **Verification strategy.** Today: compile-time checks on literals, runtime contract checks, and fuzzing (`strata verify`, with results cached by content hash). The options discussed are summarised at the end of the last conversation and in `README.md` under the roadmap.
- **Transpilation / interop** with Python (Pydantic, JSON as the boundary), TypeScript and Rust. Later.

## Known gaps

- Trait methods must take `self`; there is no `fn empty() -> Self` dispatch yet, and method names must be unique across traits.
- Exhaustiveness checking ignores refinements: a `match` on `NonEmpty` still needs a `[]` arm.
- Comments before a trait `law` are rejected (not stored yet). Blank lines inside bodies are not kept.
- No host bindings (FFI), so a real SQLite backend isn't possible yet.
- `strata verify` on the tasks example takes about 35s; the slowest part is generating `MemQueue` values that satisfy its invariants.
- `src/flow.rs` and the `flows` data in `ui.rs` are no longer used by the viewer, now that the Flow view is gone. They can be removed.

## Decided direction (user, end of session 1)

### Verification: automatic theorem prover (SMT), with layered results
- Use an SMT solver (Z3 or cvc5) as the main verifier. Contracts (refinements, `requires` / `ensures`, trait laws) become proof obligations.
- Report one of four **layered results** per obligation:
  - `proven` (the solver proved it);
  - `fuzzed` (not proven, but no counterexample found);
  - `failed` (with the solver's or fuzzer's counterexample);
  - `assumed` (trusted: outside libraries, I/O).
  A project policy can require `proven` for chosen modules.
- **Loop invariants**: infer them where possible (e.g. bounds and accumulators). Explicit `invariant …` annotations only where they really help; agents may write them and the solver checks them.
- Callers are checked against a callee's **contract**, not its body, which fits modules meeting through contracts. Cache results by content hash, as `verify` already does.
- Suggested order:
  1. integer, length and string-format refinements;
  2. `requires` / `ensures` across calls;
  3. loop invariants;
  4. trait laws.
  Fuzzing remains the fallback for whatever the solver can't decide.
- Interactive proofs (Lean/Coq-style) come later, as a bigger separate project.

### Compilation before transpilation
1. **Native compilation through LLVM** to machine code, with a garbage-collected runtime (no borrow checker in Strata). This comes before transpiling.
2. **A standard library** with batteries included. The user cited "VLAN" as inspiration; most likely the V language (vlang), so confirm with the user. Priorities:
   - basic I/O, files and the OS / processes, for CLIs;
   - networking (HTTP client and server);
   - web apps and views: HTML / CSS generation and serving, a first-class target because agents write web UIs for people;
   - agentic systems built in, e.g. LLM calls and tools as standard-library capabilities.
   These are capabilities (traits) implemented by the runtime, consistent with the decision that there are no effects; their contracts are `assumed`.
3. **Interop** with Python, TypeScript and Rust using **JSON as the boundary**: generate types and codecs (Pydantic models, zod / TS types, serde structs) from Strata types and contracts.
4. **Transpilation** to Python / TypeScript later. Rust transpilation is harder without a borrow checker (it would need a GC or reference counting in the generated code), so it has lower priority than LLVM.

### ORM built into the language (decided)
- Records map directly to tables, building on the automatic JSON serialisation and on types being structural and validated. Refinements become column checks (`CHECK` constraints) and are validated on load. `distinct` ids map to keys.
- The first backend is SQLite. The database is a capability (a trait), so tests can use an in-memory implementation or `mock`.
- Queries should be typed (checked against the record's fields), not strings.

### Long-term goal: self-hosting
The compiler should eventually be written in Strata. Gaps to close first:
- **Native compilation**: the interpreter is too slow to run a compiler.
- **Efficient mutation of collections**: in-place updates when a value has a single owner (as in Swift / Koka), instead of copying.
- **String and byte primitives**: characters, slicing and indexing.
- **Early `return`, and a `?` operator** to propagate `Result` errors.
- **A small native runtime**: GC, I/O, and binding to the SMT solver.

Bootstrap plan: write the frontend (lexer, parser, printer) in Strata and test it against the Rust one, then the checker, then compile the Strata compiler with itself and check that its output stops changing (a fixed point).

### Error handling, strings and mutation (decided)
- **`except` blocks** (V's `or` blocks, with Python's keyword) handle a failed `Result` or a missing `Option` value inline, and chain: `let cfg = load(path) except { default_config() }`. Inside the block, the error should be available under a name (to be decided, e.g. `except e { ... }`).
- **Propagation operators**: `!` propagates a `Result` error (`let x = parse(s)!`); `?` propagates an `Option` none (`let y = find(xs, p)?`). Both work in chains (`a.b()!.c()?`).
- **Strings are pure UTF-8**, with no other string types. Add good string primitives: characters, slicing, searching, splitting.
- **String interpolation in the style of Python f-strings**: `f"{name} has {len(xs)} items"`.
- **Mutation of a `mut` value happens in place**, with no copying, whenever nothing else holds the value. The hard part is knowing when that's the case: use the absence of shared references plus ownership or reference counting (as in Koka's Perceus) so updates through a mutable binding are real in-place updates.
- **Self-hosting is a long-term goal**, after the language matures. The compiler would be written in Strata, target LLVM, and use the LLVM libraries.

### Runtime and standard library (decided)
- **A small native core in Rust** (C is possible for the system-call layer; decide when we get there): the GC, the system-call / OS boundary, and bindings to LLVM and the SMT solver.
- **Most of the standard library in Strata itself**: collections, strings, the ORM, HTTP / web views, CLI helpers, and agent tooling. Only the thin OS / runtime layer is native.

### Lambda syntax (decided; not implemented yet)
- Replace the Rust-style `|x| body` with a **bare arrow**: `x -> body` and `(a, b) -> body`, with optional parameter types such as `(s: String) -> …`. For a longer body, use a block: `x -> { ... }`.
- `->` is never an operator inside expressions, so there is no ambiguity; `=>` stays reserved for `match` arms.
- **No shorthand lambdas** (no `$`, `_` or implicit parameter) and **no separate anonymous-`fn` form**.
- **Refinements keep `it`** for "the value" (`where it > 0`).
- When implementing, update the parser, printer, examples, README and tests; `render` must still round-trip.

### Requested cleanups (user, not implemented yet)
- **String concatenation uses `+`**, not `++`: `"a" + "b"`. Lists use `+` too (`[1] + [2]`, decided), so `++` goes away entirely. Update the checker (`numeric` currently rejects `String` for `+`), the interpreter, the printer and all examples.
- **Use `len` everywhere, not `str_len`.** `len` is already overloaded for `String`, so remove `str_len` from the builtins and the prelude refinements, and update the examples.
- **Bring every example up to date with the current language**: `examples/*.strata` and `examples/tasks/`, plus the README snippets. Use the prelude types (`Natural`, `Positive`, `NonEmptyString`), inline refinements where a constraint is used once, `let mut` / loops where procedural code reads better, and (once implemented) arrow lambdas.
- **GitHub syntax highlighting**: label Strata code blocks in the README and docs as ```` ```rust ````. Rust is the closest match (`fn`, `trait`, `impl`, `let mut`, `match … =>`, `///`, `->`), and GitHub's highlighter for it is solid. Also add a `.gitattributes` with `*.strata linguist-language=Rust`, so `.strata` files are highlighted on GitHub and counted as code.
