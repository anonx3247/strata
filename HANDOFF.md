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
- **No backward compatibility.** Breaking changes are expected: never add shims so that older stored trees (`.strata/` objects) or old syntax keep working. Re-import from source instead.
- **Rendered code wraps at about 80 columns** by breaking lines, not soft-wrapping.
- **Viewer**: two levels (interface/source), the code view sliced around the selection, a type map as a separate view, and no Flow view.

## Viewer UI changes (done)

Doc comments are shown as the description in the right panel (and stripped from the code, `strip_doc` in `ui.rs`); the inspector has no source block; code items are plain code with a margin marker; bodies open inline (`has_body` in the snapshot; impls and traits now have a signatures-only level 1 in `view.rs`); the header has *Code* / *Type map* tabs and the code toolbar has *expand all*. The selected definition starts open.

Still open: **type map as a side panel** instead of a full tab. The user suggested it; ask before doing it.

Relations (decided): a definition has **dependencies** (what it mentions) and **references** (what mentions it). Implementing a trait is a separate relation, never a reference or a dependency: an `impl T for X` block is listed under *Implements* on `X` and *Implementations* on `T`, and its own trait and type are shown as *Trait* / *For*. The snapshot carries `deps`, `refs`, `impls` and `impl_of`. The type map shows dependency edges by default; *implementations* and *refinement types* are opt-in. There are no effects and no functions in the type map. The user wondered about a traits-only view, showing how traits work together; not decided.

## Decided with the user: git, diffs and review

- **Git stays the base.** Commit the `.strata` text files and the log (`log.jsonl`, append-only, entries with ids, `merge=union` in `.gitattributes`). Objects and index become a local cache rebuilt from the files. Optionally commit the verification cache.
- **Structural diff** between two git revisions: load both snapshots, pair definitions, compare trees. The log entries present at HEAD but not at the base are the atomic steps in between (for stepping through, renames, messages). A PR = merge-base..HEAD.
- **Change kinds**: added/removed; renamed/moved (one line, "N references updated", folded); signature change marked in place, with contract stronger/weaker; body change shown in place with unchanged statements folded, **falling back to a line diff of the canonical text when most of the body changed**; type field/variant changes (with "N matches must handle it"); trait changes; doc comments as a word diff; verification before → after. Ordered types → interfaces → implementations → tests.
- **`strata rename old new`**: done (`resolve::rename`). It reuses the resolver's scoping, so locals shadow and `use` imports work as the checker sees them, and it refuses a rename that would change what any other name refers to. Functions, types and traits only: variants, fields and trait methods can't be renamed yet, and docs and comments aren't rewritten (the command lists the ones that mention the old name). The log entry has `"rename": {"from", "to", "references"}`. Database only for now (like `edit`).
- **Merge driver**: `strata merge-file %O %A %B %P` per definition, then type-check. GitHub won't run it, so: `strata sync main` merges main into the branch locally, branch protection requires branches to be up to date, and a required CI check runs `strata check` + canonical-format check on GitHub's merge ref.
- **jj**: compatible (runs on git) but not a dependency.
- The tour is later. The user wants to settle the look first: offer a static mockup of the *Changes* view.

Order: ~~`strata rename`~~ → ~~diff core~~ → ~~`strata diff`~~ → ~~*Changes* view~~ → merge driver.

**Built:** `src/diff.rs` (pairing through renames, words per change, line diff with in-place marks and folds that keep enclosing headers, line-diff fallback at ≥60% changed, contract notes, rename references, verification before/after), `src/history.rs` (a codebase's log replayed with `Codebase::states`, one step per patch; or git: `BASE..HEAD` commits plus the working tree, read with `git show`), `strata diff [--from N] [--to N] [--step K] [--json]` / `strata --file DIR diff [BASE]`, `/api/changes` and the viewer's *Changes* tab. The mockup is still in `design/changes-mockup.html`.
Not yet: the log is not committed to git yet, so git ranges have no renames or patch steps inside commits; trait-method/variant renames; "N matches must handle the new variant"; moves between modules. Closure hashes (verification cache keys) now ignore docs and comments, and interface hashes ignore docs.

## Open topics the user wants to discuss and decide on (do not implement without agreement)

- **Verification strategy.** Today: compile-time checks on literals, runtime contract checks, and fuzzing (`strata verify`, with results cached by content hash). The options discussed are summarised at the end of the last conversation and in `README.md` under the roadmap.
- **Transpilation / interop** with Python (Pydantic, JSON as the boundary), TypeScript and Rust. Later.
- **Strata IDE**: the viewer as a standalone desktop app (Tauri; the backend is already Rust), speaking ACP (Agent Client Protocol) so agents such as Claude Code or Codex run inside it, with a Strata MCP server giving them structural tools (slices, check, edit, rename). The viewer would show live where each agent is working (log entries tagged with the agent's session). Idea stage.


## Known gaps

- Trait methods must take `self`; there is no `fn empty() -> Self` dispatch yet, and method names must be unique across traits.
- Exhaustiveness checking ignores refinements: a `match` on `NonEmpty` still needs a `[]` arm.
- No host bindings (FFI), so a real SQLite backend isn't possible yet.
- `strata verify` on the tasks example takes about 35s; the slowest part is generating `MemQueue` values that satisfy its invariants.
- Typed lambda parameters (`(n: Positive) -> …`) are checked statically but their refinements are not enforced at runtime yet.
- `!` does no error conversion: the function's `Result` must have the same error type. f-strings have no format specs (`{x:.2}`).

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
2. **A standard library** with batteries included. The user cited the V language (vlang) as inspiration (confirmed). Priorities:
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
- **Done**: `except` blocks (`e except { fallback }`, `e except err { ... }`, the fallback may `return`), `!` / `?` propagation, early `return`, f-strings, and string primitives (`chars`, `get`, `slice`, `index_of`, `replace`, `lines`, `repeat`, `reverse`, case, `is_digit` / `is_alpha` / `is_space`, `ord` / `chr`). Strings are indexed by character. See the README section "Errors".
- **Mutation of a `mut` value happens in place**, with no copying, whenever nothing else holds the value. The hard part is knowing when that's the case: use the absence of shared references plus ownership or reference counting (as in Koka's Perceus) so updates through a mutable binding are real in-place updates.
- **Self-hosting is a long-term goal**, after the language matures. The compiler would be written in Strata, target LLVM, and use the LLVM libraries.

### Runtime and standard library (decided)
- **A small native core in Rust** (C is possible for the system-call layer; decide when we get there): the GC, the system-call / OS boundary, and bindings to LLVM and the SMT solver.
- **Most of the standard library in Strata itself**: collections, strings, the ORM, HTTP / web views, CLI helpers, and agent tooling. Only the thin OS / runtime layer is native.

### Unions, variants as types, and match patterns (decided 2026-11-19)
- **`Option` and `Result` stay as they are**, and so do `!` / `?` propagation. No `T?` / `T ! E` syntax.
- **Unions are sets of values**: `A | B` holds any value of `A` or of `B`. Unions normalize: `Int | Positive` is `Int`; `(Int where p) | (Int where q)` is `Int where p || q`; different bases stay a union. `A` fits `A | B`; `A | B` fits `C` when both do. Type parameters can be members (`T | E` with bounds); if they end up the same type, it collapses, harmlessly.
- **Variants are types.** `type ShapeError = | NonEmptyErr | TooBig { max: Int }` declares the types `NonEmptyErr` and `TooBig`, and `ShapeError` is `NonEmptyErr | TooBig`. So `Result[Circle, NonEmptyErr]` or a parameter of type `TooBig` are allowed.
- **Names are unique across the project**: no variant may share its name with a type or with another variant. A clash between modules is fixed by renaming on import (`use shapes.{Circle as Round}`).
- **No automatic wrapping**: always write `Ok(…)`, `Err(…)`, `Some(…)` (`Result[Int, Int]` would be ambiguous otherwise).
- **Patterns are sets of values**: a type name (`Positive =>`, `Square =>`), a variant as today, a literal, a condition (`where 3 <= it && it <= 5 =>`, base type = the scrutinee's), `_`. An arm takes what no earlier arm took. Exhaustive = the scrutinee's (normalized) type is covered by the arms; an arm already covered by earlier ones is an error (unreachable).
- **Narrowing, no new name**: `match x { Positive => { x … } }` narrows `x` itself in the arm. For an expression: `match f(y) as x { … }`. Field paths (`o.status`) narrow too. Narrowing of a `let mut` lasts until it is assigned.
- **Coverage**: integer (and `len(it)`) comparisons with constants, combined with `&&` / `||`, are decided exactly as interval sets. Anything else waits for the SMT solver; until then such an arm needs a later `_`. This also lets a match on `NonEmpty` skip the `[]` arm.

### Lambda syntax (done)
- Replace the Rust-style `|x| body` with a **bare arrow**: `x -> body` and `(a, b) -> body`, with optional parameter types such as `(s: String) -> …`. For a longer body, use a block: `x -> { ... }`.
- `->` is never an operator inside expressions, so there is no ambiguity; `=>` stays reserved for `match` arms.
- **No shorthand lambdas** (no `$`, `_` or implicit parameter) and **no separate anonymous-`fn` form**.
- **Refinements keep `it`** for "the value" (`where it > 0`).
- When implementing, update the parser, printer, examples, README and tests; `render` must still round-trip.

### Requested cleanups (done)
- **String concatenation uses `+`**, not `++`: `"a" + "b"`. Lists use `+` too (`[1] + [2]`, decided), so `++` goes away entirely. Update the checker (`numeric` currently rejects `String` for `+`), the interpreter, the printer and all examples.
- **Use `len` everywhere, not `str_len`.** `len` is already overloaded for `String`, so remove `str_len` from the builtins and the prelude refinements, and update the examples.
- **Bring every example up to date with the current language**: `examples/*.strata` and `examples/tasks/`, plus the README snippets. Use the prelude types (`Natural`, `Positive`, `NonEmptyString`), inline refinements where a constraint is used once, `let mut` / loops where procedural code reads better, and (once implemented) arrow lambdas.
- **GitHub syntax highlighting**: label Strata code blocks in the README and docs as ```` ```rust ````. Rust is the closest match (`fn`, `trait`, `impl`, `let mut`, `match … =>`, `///`, `->`), and GitHub's highlighter for it is solid. Also add a `.gitattributes` with `*.strata linguist-language=Rust`, so `.strata` files are highlighted on GitHub and counted as code.
