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

- **Git stays the base** (built). A package's module files and its `strata.log` are committed; `.strata/` is only the local verification cache. `edit` / `add` / `rename` rewrite the files and append an entry (id, message, changes with each definition's text before and after, `rename`). The log merges by union. `strata init` writes `strata.toml` and an empty `strata.log`, so the log is tracked from the start (`git commit -am` would otherwise leave it behind).
- **Structural diff** between two git revisions: load both snapshots, pair definitions, compare trees. The log entries present at HEAD but not at the base are the atomic steps in between (for stepping through, renames, messages). A PR = merge-base..HEAD.
- **Change kinds**: added/removed; renamed/moved (one line, "N references updated", folded); signature change marked in place, with contract stronger/weaker; body change shown in place with unchanged statements folded, **falling back to a line diff of the canonical text when most of the body changed**; type field/variant changes (with "N matches must handle it"); trait changes; doc comments as a word diff; verification before → after. Ordered types → interfaces → implementations → tests.
- **`strata rename old new`**: done (`resolve::rename`). It reuses the resolver's scoping, so locals shadow and `use` imports work as the checker sees them, and it refuses a rename that would change what any other name refers to. Functions, types, traits, **variants, trait methods** (the trait, every impl's method and the calls) and **fields** (`strata rename Line.qty quantity`: declaration, accesses, record literals, updates, patterns). Variants and methods go through the resolver like definitions (`resolve::member` / `owner` / `defined`; an impl's methods are named in the trait's module, which rename mode used to get wrong). Field accesses are told apart by type, not by name: when another record (or the prelude, or `.value` of distinct types) has a field of that name, `resolve::rename_field` tags each access in a copy (`qty·d3k0`), and the checker (`types::field_owners`, a thread-local probe in `infer` for tagged `Expr::Field`) records the record type(s) of each receiver; only the owner's accesses are renamed, and an access on a union of the owner and another record is refused. Literals, updates and patterns are matched by their constructor. It rewrites the serialized tree in one fixed-order walk (tags and renames line up), and the command type-checks the result.
- **`strata move <name> <module>`** (`resolve::move_def`): a function, type or trait (with its variants or methods) changes module, keeping its short name. The resolver's write-back has a move mode (`run_with(.., moves)`): qualified references get the new module (`orders.charge(o)` → `billing.charge(o)`), short ones stay short and their module gets `use billing.{charge}`, `use` items that named it go (emptied lists are removed), and the moved definition's own short references get imports where it goes. Refused when something private would be out of reach (a private dependency, or a private definition used from its old module), or when any name would resolve differently (the same check as rename). Logged as `"renames": [{.., "kind": "move"}]`; diffs say "moved from `orders`"; `sync` follows moves (`resolve::follow_move`, compared by name since imports change the definitions' positions). Docs and comments aren't rewritten (the command lists the ones that mention the old name). Several renames are one step (`strata rename A.tag label B.tag label`, `resolve::rename_all`): fields are renamed together by `rename_fields` (an access on a union is renamed when every record it reads from is renamed to the same name), then the rest one after the other; one type-check, one log entry. The log entry has `"renames": [{"from", "to", "kind"}], "references"` (`store::renames_of` reads it; `strata move` writes `kind: "move"`). It works on the package's files and logs the rename.
- **Merge driver**: `strata merge-file %O %A %B %P` per definition, then type-check. GitHub won't run it, so: `strata sync main` merges main into the branch locally, branch protection requires branches to be up to date, and a required CI check runs `strata check` + canonical-format check on GitHub's merge ref.
- **jj**: compatible (runs on git) but not a dependency.
- The tour is later. The user wants to settle the look first: offer a static mockup of the *Changes* view.

Order: ~~`strata rename`~~ → ~~diff core~~ → ~~`strata diff`~~ → ~~*Changes* view~~ → ~~merge driver~~.

**Built:** `src/diff.rs` (pairing through renames, words per change, line diff with in-place marks and folds that keep enclosing headers, line-diff fallback at ≥60% changed, contract notes, rename references, verification before/after), `src/history.rs` (a package's log replayed with `Codebase::states`, backwards from the files so hand-written code before the first entry is the start, one step per patch; or git: `BASE..HEAD` commits plus the working tree, each commit split into the log entries it added, replayed forwards with `store::apply`, plus a step "edits outside strata" for what the log doesn't explain), `strata diff [--from N] [--to N] [--step K] [--json]` / `strata --file DIR diff [BASE]`, `/api/changes` and the viewer's *Changes* tab. The mockup is still in `design/changes-mockup.html`.
**Merge driver (built):** `src/merge.rs` and `strata merge-file` / `setup-git` / `sync` / `ci`, plus packages (`src/package.rs`, see below). `strata ci` finds every package (folder with a `strata.toml`) and script (a `.strata` file outside every package) with `git ls-files`. It checks each one and validates `exports`, and it requires canonical format: `printer::program` of each file parsed alone. The examples were rewritten into canonical form for it (`strata ci --fix`).
How the driver works: definitions are paired by qualified name, and one-sided changes win. Two-sided changes go through `git merge-file` on the definition's canonical text, and the result is kept only if it parses back into that one definition; otherwise there are diff3 markers around it. Renames are detected without the log: a definition gone, and an added one with the same kind and module whose text is ≥60% similar to the base's after `resolve::rename`. The rename is applied to the base and to the other side. After the merge, the file is type-checked inside its package, with the package's other files read from disk (ours, because merge-ort runs drivers before it writes the worktree). Only errors that neither side has alone fail the merge. The whole-program check after all files are merged is `sync`'s `ci` run.
`sync` passes the driver with `-c merge.strata.driver=...` and a temporary `core.attributesFile`, so it works without `setup-git`. It merges `origin/BRANCH` when there is a remote, commits only if `ci` passes, and otherwise leaves the merge staged.
CI: `.github/workflows/strata.yml` (`strata ci` on the PR merge ref, plus `cargo test`). The repository is public, and `main` is protected: `strata ci` and `cargo test` are required checks and branches must be up to date (`strict`). Admins are not enforced, so the owner can still push to `main` directly; everyone else goes through a PR.
**Renames across a merge (built):** `sync` runs the driver with `STRATA_MERGE_BASE` / `STRATA_MERGE_THEIRS`; then the driver skips its per-file type check (while git merges, the other files are still ours). After git is done, `merge::follow_renames` reads the renames both sides logged since the merge base and applies each to the merged package (`resolve::follow`): the renamed definition is put back under its old name (and `use` items naming the new name name the old one), every reference to the old name is renamed, and each definition must resolve as before (what the rename touched, with old → new) or exactly as merged (everything else), so a local that would capture the new name makes it refuse. The fix is a logged change (`"follows"`), staged into the merge commit; then `ci` checks everything.
Not yet: following renames under a plain `git merge` (only `sync` does it); a refused follow leaves every reference to the old name for `ci` to report; moving several definitions at once (move a private helper with its user: make it `pub`, or move it first); a rename+edit is recognised only when it is ≥60% similar *and* callers switched to the new name (tiny functions look alike, so similarity alone gave false renames).

Diff: a renamed variant, method or field shows on its type or trait as "variant `Circle` renamed to `Round`" ("only a name changed · N references updated"), and code that only follows it is folded; other changes to the owner are computed against the old owner with the rename applied. `sync` follows member renames too (`resolve::set_member_name` puts the member back under its old name first). Diff, not yet: "N matches must handle the new variant"; moves between modules. Closure hashes (verification cache keys) now ignore docs and comments, and interface hashes ignore docs.

## Packages, modules and dependencies (decided with the user)

- **A package is a folder with a `strata.toml`**: `name` (required; `strata init` writes the folder's name, and it stays when the folder moves, e.g. in a worktree), `exports` (optional, modules or single definitions, the equivalent of `pub use`; default every `pub`), `[deps]`, `[verify]` (verifier policy, e.g. modules that must be `proven`). There is **no `lib.strata`**. **`main.strata`** is the root module and the default entry point. A `.strata` file outside every package is a script. A nested folder with its own `strata.toml` is a separate package.
  **Built:** `name`, `exports` (checked by `ci`), discovery, nested packages, `init`. `[deps]` and `[verify]` are parsed but unused.
- **A file is a module; `module m { }` blocks are gone** (built). The parser has no `module` form, `printer::program` is one file's text, and `render` prints one module (`render MODULE`, `main` for the root) or `--split DIR`; on several modules without either it says so. A view across modules labels each group with a `// ── module m` comment line. Where a definition sits inside a file doesn't matter, and the standard rendering is one file per module. Folders organise modules freely; there are no submodules with special access. Still to confirm: whether a module's name is its path inside the package (`shop/orders.strata` → `shop.orders`, as today; recommended, so names don't collide and moving a file is a module rename) or the file name alone.
- **Dependencies from any git URL** (Go-like, not only GitHub), declared in `[deps]` (`json = "https://…"`, or `{ path = "../x" }` in the same repo), used as `use json.parse`. Fetched into a global cache and run directly, with no install step. Proposed, not yet confirmed: a generated `strata.lock` pinning each dep to a commit + content hash, and `strata update`. Semver tags (explained to the user) can come later; Strata could compute the right bump from interface hashes and contract diffs. Capabilities-as-values mean a dependency can't do I/O unless handed a capability, which makes URL imports much safer than npm/Go.

## Open topics the user wants to discuss and decide on (do not implement without agreement)

- **Complexity / cost.** Separate from termination: a function can terminate and still take exponential time (naive `fib`, Ackermann). The user raised it as "another issue"; nothing is designed yet.
- **Transpilation / interop** with Python (Pydantic, JSON as the boundary), TypeScript and Rust. Later.
- **Storage: hash references, and propagation** (discussed 2026-11-20; direction agreed, not built). The user prefers storing definitions by hash with references by hash and names in a separate table (Unison's model: renames become free, merges of new code can't conflict, several agents can add code at once), over a binary encoding of the same files. On an edit, dependents follow automatically when they still type-check ("option 1"), and otherwise get a to-do. The user's addition: while a to-do is open, the references to the old version are replaced with typed holes, so the agent sees exactly what to fill in. Before building: settle how a hole keeps what it replaced (the old call, for the agent), and that code with holes still checks but can't run.
- **Storage: text files or the tree** (user, 2026-11-20). The AST is the language; the current syntax is one front end, and others (Nim-, Python-, C-, Haskell-like) should be possible, so storing canonical text is not essential (a compact tree encoding is an option, maybe not worth it). The worry is agents editing the files directly instead of using `edit` / `rename`, which check every change and answer immediately. If every edit goes through the tools (type-safe, atomic), several agents (the user guesses 3–4) could work in the same codebase at once without conflicts; that would need a cache and no rendered files as the source. "We'll see then."
- **Strata IDE**: the viewer as a standalone desktop app (Tauri; the backend is already Rust), speaking ACP (Agent Client Protocol) so agents such as Claude Code or Codex run inside it, with a Strata MCP server giving them structural tools (slices, check, edit, rename). The viewer would show live where each agent is working (log entries tagged with the agent's session). Idea stage.


## Known gaps

- Trait methods must take `self`; there is no `fn empty() -> Self` dispatch yet, and method names must be unique across traits.
- No host bindings (FFI), so a real SQLite backend isn't possible yet.
- `strata verify --force` on the tasks example takes a few seconds (proven functions are not fuzzed); the slowest part is still generating `MemQueue` values that satisfy its invariants.
- `!` converts errors only by fitting (a variant into its sum, a member into a union), not with a conversion function.

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

**Built (step 1 and most of 2)**: `src/smt.rs` and `verify::check_fn`. See README › Verification. Results: `Proven { checks }`, `Fuzzed { cases, discarded, proven, unproven }` (each unproven check with its reason), `Counterexample { .., found_by: solver|fuzzer }` (a solver model is replayed in the interpreter first, and counts only if it really fails), `Incomplete`. Z3 through SMT-LIB text over a pipe (`z3 -in`, `rlimit` for determinism, solver version in the cache key); `verify --smt` prints the queries (termination ones too), `--checks` lists proven checks. Cross-check: proven functions that terminate are fuzzed a little (`--cross-check`, step budget per run; past it, a run proves nothing); a failure there is reported as a prover bug. Policies: `[verify]` in `strata.toml` (`proven`, `terminates`, `no_recursion`, each a list of modules) and `--require`. Calls already use the callee's contract (requires checked, ensures assumed), and the same call with the same arguments gets the same result (memo), so refinements re-evaluated in assumptions and goals line up. Impl methods are proven against their trait's contract; laws are still fuzzed only.
Not yet: `assumed` (no outside code exists yet); loop invariants (loops forget what they change); list contents (a list's elements are known only as "all valid values of type T" when it crossed a checked boundary or came from `map` with a named function); floats, dicts, sets; `sum` is modelled only as "≥ 0 for non-negative elements, 0 when empty", `all`/`any` only when empty; generic callees lose precision (a result `MemQueue[P, E]` doesn't satisfy a `MemQueue[Int, String]` check, because element types are compared syntactically); regexes with `\d`, `\w`, `\s` or flags are not translated (Rust's are Unicode-aware); values of a union type (`Square | Circle`) are unknowns, so checks on them are fuzzed (variants used as types, or-patterns, `where` patterns and type-name patterns are modelled); cvc5 is not wired in (`STRATA_SOLVER` picks the binary, but the flags are Z3's).

### Termination and helpers (decided with the user, session 3; built)
- Termination is reported for every function: terminates (with how) / may not terminate (with why) / assumed (`terminates` marker, after the contract). See README › Verification. The viewer shows "may not terminate" in orange; diffs show it in the verification word.
- **No `decreases` clauses** for now (user: a `for` loop covers the case where you can name the bound). Instead the bound is inferred. For `while a < b`, the solver tries `b - a` (and the other comparisons). For recursion, a smaller piece of a parameter is detected first. Otherwise candidate measures are tried (`termination::candidate_measures`): integer parameters, `len` of list or string parameters, both differences of each comparison of parameters in the body, then pairs compared in order (Ackermann). Not covered: mutual recursion, measures built from more than a difference (`n * n - k`), recursion through a lambda.
- **Helpers are seen through in contracts** when they terminate and cannot fail (`termination::analyze` → `transparent`): no `requires`, no refined parameters, no `mut`, no division by a non-literal, no `assert`, no hole, no refined constructors, and only such helpers or total builtins called. A helper whose termination is only assumed is still seen through, and the proof lists it. In function bodies, calls are still checked against contracts only.
- Policies: `--require terminates` accepts an explicit `terminates`; `--require no-recursion` rejects any recursion, even structural.

### Integers and floats (decided, user, session 3)
- **`Int` is a big integer that is a machine integer while it fits** (like Python). Built: `Value::Int(i64)`, and `Value::Big(BigInt)` only for a value that doesn't fit (so equal numbers are equal values); arithmetic switches over on overflow, and a literal too large for 64 bits is `Expr::BigInt(digits)`. Integers beyond 64 bits are written to JSON as strings of digits (serde_json can't write larger numbers without `arbitrary_precision`, which breaks deserialising the internally tagged verdicts); they are read from a number up to 2^64 - 1 or a string. An integer pattern must fit in 64 bits.
- **Sized types live in the standard library as refinements of `Int`**: built in the prelude, `I8` … `I64`, `U8` … `U64`. They are verified like any refinement, and LLVM compiles each to its own width.
- **`Float` is `F64`**; `F32`, `BF16` and similar exist alongside, with their own LLVM representations. (Floats are not modelled by the prover yet; SMT-LIB has a floating-point theory for when they are.)

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

### Unions, variants as types, and match patterns (decided 2026-11-19, built)
- **`Option` and `Result` stay as they are**, and so do `!` / `?` propagation. No `T?` / `T ! E` syntax.
- **Unions are sets of values**: `A | B` holds any value of `A` or of `B`. Unions normalize: `Int | Positive` is `Int`; `(Int where p) | (Int where q)` is `Int where p || q`; different bases stay a union. `A` fits `A | B`; `A | B` fits `C` when both do. Type parameters can be members (`T | E` with bounds); if they end up the same type, it collapses, harmlessly.
- **Variants are types.** `type ShapeError = | NonEmptyErr | TooBig { max: Int }` declares the types `NonEmptyErr` and `TooBig`, and `ShapeError` is `NonEmptyErr | TooBig`. So `Result[Circle, NonEmptyErr]` or a parameter of type `TooBig` are allowed.
- **Names are unique across the project**: no variant may share its name with a type or with another variant. A clash is fixed by renaming one of them (there is no renaming on import).
- **No automatic wrapping**: always write `Ok(…)`, `Err(…)`, `Some(…)` (`Result[Int, Int]` would be ambiguous otherwise).
- **Patterns are sets of values**: a type name (`Positive =>`, `Square =>`), a variant as today, a literal, a condition (`where 3 <= it && it <= 5 =>`, base type = the scrutinee's), `_`. An arm takes what no earlier arm took. Exhaustive = the scrutinee's (normalized) type is covered by the arms; an arm already covered by earlier ones is an error (unreachable).
- **Narrowing, no new name**: `match x { Positive => { x … } }` narrows `x` itself in the arm. For an expression: `match f(y) as x { … }`. Field paths (`o.status`) narrow too. Narrowing of a `let mut` lasts until it is assigned.
- **Coverage**: integer (and `len(it)`) comparisons with constants, combined with `&&` / `||`, are decided exactly as interval sets. Anything else waits for the SMT solver; until then such an arm needs a later `_`. This also lets a match on `NonEmpty` skip the `[]` arm.
- **Built** (branch `unions`): `Ty::Union` normalized by `Checker::union_of`; subsumption in `fits` (used by `expect`: union members, variants into their sum; type arguments stay invariant); `infer_at` passes the expected type through `if` / `match` / blocks / lists / constructors; `pattern` returns the narrowed type and a value set `P`; exhaustiveness and unreachable arms use atoms (`atoms`, `covers`, `missing`, `useful`) with integer intervals in `types::ints`. Interpreter: `value_in` / `shape_fits` for type patterns and `is`, unions in `check_type` and JSON input, impls on variants. Since then (decided with the user): or-patterns; unions inferred for alternatives when no expected type decides; type arguments fit member-wise (values are never shared) except for arguments to `mut` parameters, which must be exact; `!` passes errors into wider error types; length (`len(it)`) and `Float` conditions are decided (`types::reals` tests one point per region); the matched value's written refinement counts (`Coverage.excluded`), taken from variables' declared types (mutable ones too: the interpreter checks every assignment and write-back against the declared type, `Frame::declared`), their fields and function results (`Checker::written`). Narrowing in an arm is skipped for a `mut` variable the arm assigns (`Checker::assigns_to`), so loops stay sound. A `mut` argument's declared refinement must contain the parameter's (`refinement_within`); assignments of literals are checked at compile time (`literal_outside`). Not yet: list lengths other than empty / not empty; conditions beyond constant comparisons (the SMT branch has the pieces, see below).

### Match coverage with the SMT solver (assessed 2026-11-19, not started)
The `smt-verification` worktree (uncommitted, on old main) already has: symbolic values for parameters with their refinements as facts (`havoc`), `Vc::pattern` turning a pattern into an SMT condition on the matched value, per-arm path conditions in `Expr::Match`, obligations solved with Z3. To decide coverage of undecidable conditions it still needs: (1) an obligation per `match` that some arm holds (the `or` of the arm conditions, under the facts), and one per arm that it can hold (unreachable when proven impossible); (2) the new pattern forms in `Vc::pattern`: type names, `where`, or-patterns, and `Match`'s `as` name (it predates the `unions` branch, so merge first); (3) a policy between `check` and `verify`: `check` would report such a `match` as "needs proof" instead of an error, and `verify` proves it, reports a counterexample, or leaves it runtime-checked (a `no match arm` fault). Floats are not modelled there; the checker's own `reals` covers them.

### Mutation at the call site, references, and unproven checks (decided 2026-11-19)
- **`mut` at the call site (done)**: an argument to a `mut` parameter is written `f(mut x)`; missing it, or writing it for a parameter that is not `mut` (or a builtin, or a closure), is an error. A `mut self` method's receiver is not marked (`queue.claim(2)`), as in Swift; the receiver must still be a mutable variable. `Expr::Mut`, checked in `check_mut_args`.
- **References**: immutable values can always be shared (no copying in the compiled form); a mutable reference only exists as a `mut` parameter (copy-in/copy-out today, in place later). Recommended to the user: keep references second-class (never stored or returned), like Swift `inout` / Hylo, since first-class ones would need a borrow checker; a possible local form is `let mut l = mut order.lines[0]`. Not decided beyond the call-site marker.
- **Unproven checks are reported by default** (with the runtime check kept); a project policy can require `proven` for chosen modules. For the SMT work: an assignment to a refined variable is one more obligation (e.g. `x = f(x) - 3` with `x: Positive` needs `f`'s contract to imply it).

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
