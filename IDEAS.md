# Ideas, decisions and open questions

Everything that keeps track of where Strata is going: open questions, ideas, what has been decided (and whether it is built), known gaps and the roadmap. The [README](README.md) and [docs](docs/) describe what is built.

## Open questions (to discuss before building)

- **Slicing with Python's syntax** (user, 2026-11-20; idea, not designed). Lists and strings are sliced the same way: `xs[1:3]`, `xs[2:]`, `xs[:-1]`, `xs[::2]`, `s[1:4]`. To decide: whether it replaces `slice` / `take` / `drop`, what `xs[i]` returns (an `Option`, or a runtime check against `len`), and negative indices.
- **What iterating a string yields** (user, 2026-11-20; undecided). Bytes, code points (Go's runes; what `chars` gives today), or grapheme clusters (what a reader calls a character). To be decided together with slicing, since a slice's indices count the same units.
- **Regex verification scope** (decided with the user 2026-11-20; not built). By default, regex refinements are proven over ASCII strings only, and the result always says so ("proven for ASCII strings"). Such a proof counts as proven in an ordinary `verify` run. Under a `[verify] proven = [...]` rule it does not, unless the project opts in to accepting ASCII proofs; it is still reported as proven for ASCII either way. Full Unicode proofs are asked for explicitly where they matter (per module in `[verify]`, or per function). The interpreter stays fully Unicode either way. Pairs with "two solvers": ASCII classes are small enough for Z3 to decide exactly; full-Unicode proofs can go to cvc5, which decides the large classes.
- **Two solvers, each for its strengths** (user, 2026-11-20; direction). Z3 and cvc5 are both wired in (`STRATA_SOLVER`). Use both: route a query by what it contains (cvc5 for strings and large Unicode regex classes, which it decides exactly where Z3 times out; Z3 elsewhere), or run both and take the first definite answer.
- **Complexity / cost.** Separate from termination: a function can terminate and still take exponential time (naive `fib`, Ackermann). The user raised it as "another issue"; nothing is designed yet.
- **Transpilation / interop** with Python (Pydantic, JSON as the boundary), TypeScript and Rust. Later.
- **Changes are git commits** (built, `git_commit` in main.rs, asked for by the user). After a logged change (`record`: edit / rename / move / add) and at `strata done`, the package's files are committed with the change's message (`git add -A -- .` and `git commit -- .` in the package folder, so other staged work stays out). Nothing during a pending change. Off with `auto_commit = false` in strata.toml or `--no-commit`; skipped outside git and during a merge in progress; a failed commit leaves the change made and says why. Tests that commit by hand after strata commands no longer need to; the log-splitting test uses `--no-commit` to keep several entries in one commit.
- **Editing in an editor** (built, `edit_in_editor` in main.rs). `strata edit [MODULE | SELECTOR]` at a terminal with nothing piped in (or with `--editor`) opens, in `$VISUAL` / `$EDITOR` / git's editor / `vi` (through `sh -c`), the whole package with no argument (one file per module, all given to the editor), a module (`shop`, `main`, or its file `shop/orders.strata`, which may be new; a module's name wins over a definition's, and a definition is named with its module: `shop.total`, `main.main` for the root module's `main`), or the definitions a selector picks (one file per module they are in). Piped, `strata edit shop` puts the definitions into `shop`; there is no `--module` for `edit`. What is saved goes through `apply_edit` like a piped edit; definitions deleted from the buffer are removed. If it doesn't parse or check, the editor reopens with the errors on top (`// ✗ ` lines, dropped when read back); saving unchanged gives up. Errors go on top of the file of the definition they are in.
- **Removing or adding variants in a pending change** (built, `fix_matches` in pending.rs). Patterns can't be holes, so `match`es are fixed first: arms of removed variants are dropped (or only their alternative in an `a | b` pattern; nested ones like `Some(Pending)` too), a `match` on the changed type without a catch-all gets an arm `NewVariant => ?pending_N` (`Lost(_)`, `Lost { .. }`), and a `match` with no arm left becomes a hole. The edit prints what was dropped. Then expression holes as before (the expression walk skips patterns). Not yet: `let` / `for` patterns naming a removed variant still reject the edit.
- **Pending changes** (built, `src/pending.rs`). An `edit` whose own definitions check but that breaks other code isn't rejected: each broken use in the erroring definitions (a call, reference, constructor, method call or `is` test naming a definition whose interface changed, outermost first) becomes `?pending_N`, recorded with what it was and why, and the package enters a pending change (`.strata/pending.json`: the files before, the holes, the renames). Edits, renames and moves during it only write files (`record` in main.rs); `strata status` lists the holes; `strata done` logs the whole change as one entry (from the files before to now, with its renames), refusing while its holes are open unless `--keep-holes` (they become `?todo_N`); `strata abort` restores the files. `strata ci` refuses `?pending_*` holes, so half-done work isn't merged. Not yet: one pending change per package; a use the edit broke compatibly in an erroring definition is also turned into a hole.
- **Storage: hash references, and propagation** (discussed 2026-11-20; direction agreed, not built). The user prefers storing definitions by hash with references by hash and names in a separate table (Unison's model: renames become free, merges of new code can't conflict, several agents can add code at once), over a binary encoding of the same files (the encoding itself is now decided: see the next item). On an edit, dependents follow automatically when they still type-check ("option 1"), and otherwise get a to-do. The user's addition: while a to-do is open, the references to the old version are replaced with typed holes, so the agent sees exactly what to fill in. Before building: settle how a hole keeps what it replaced (the old call, for the agent), and that code with holes still checks but can't run.
- **Storage: the tree, in binary** (decided with the user 2026-11-20; the format is built, storing packages in it is not). Code is stored as the syntax tree, not as text: the AST is the language, and the current syntax is one front end among possible others (Nim-, Python-, C-, Haskell-like). The encoding is a compact binary format, not JSON (no serialization round-trips; big integers stored exactly). Ideally a module's `.strata` file *is* that binary, and text is rendered only when needed: `strata edit` already opens a module or definitions as text in the editor and parses what is saved, `render` / `view` print it. Rendered text files, if kept at all, are a read-only view that git ignores. Every change then goes through the tools (`edit`, `rename`, `move`), which check it and answer immediately; with a cache, several agents (the user guesses 3–4) could work in one codebase at once. Before building, settle:
  - ~~the format~~ **built**: our own, not a crate such as `postcard`, because Strata should be able to read and write it itself once bootstrapped (user). `src/binary.rs`, specified in `docs/binary-format.md`. LEB128 integers (zigzag for signed), one tag byte per enum variant, fields in order, strings by index into a table; canonical; corrupt input is an error, never a crash; no compatibility across format versions.
  - **Module files with a header** (built, branch `binary-header`, discussed with the user 2026-11-20: "header files from first principles, but inside the file"). One `.stb` per module (`strata pack DIR`), or a bundle of them (`strata pack app.stb`). The header lists each definition's name, kind, `pub`, hash, interface hash, code hash, dependencies and size; each definition is self-contained (its own strings) and stores its interface (bodies as `()`) before its bodies. `store`'s hashes are now those of the binary encoding (no more printing to hash). Measured on `examples/tasks`: all headers 14 µs, all interfaces 82 µs, everything 166 µs (with each definition's hash checked), against 735 µs to parse the text. Files are larger than the first single-file format (27 KB vs 14 KB; 20 KB of source): three 32-byte hashes per definition, and strings repeated per definition. Not done yet: the tools don't use the header (they decode everything, then recompute hashes and the graph); see the next steps.
  - **Next steps, agreed with the user:** (5) read module files in parallel; (6) cache derived results (inferred types, check results) in `.strata/` by hash, and check a module against the interfaces of the modules it uses, re-checking dependents only when an interface hash changes; (7) a long-running process that loads once and reloads only files whose hash changed. Also: interned names in the AST (`Symbol(u32)` instead of `String`), only if profiling shows string handling is the cost.
  - **Faster decoding, after a fast-lexer article** (user sent https://xnacly.me/posts/2025/fast-lexer-strategies/, 2026-11-20; ideas, not built). Decoding runs at about 160 MB/s today, against that lexer's 580–850 MB/s; the time goes into allocation (a `Box` per node, a `String` copy per name) and checking each definition's hash. What carries over: (a) **interned names**: the AST holds `Symbol(u32)` instead of `String`, so decoding a name reads an integer, names compare and hash as integers, and the file's string table maps to symbols once per distinct string (the article's zero-copy string windows and per-atom hashing); (b) **a built-in string table** shared by every file (`Int`, `String`, `List`, `Option`, `self`, `it`, …) with fixed indices, like its prehashed keywords: shorter files, and those names are known without a lookup; (c) **an arena / flat AST** (nodes in arrays, children as `u32` indices): one allocation per module, free in one go (the article: parsing 2.1x faster, freeing 26x); the most invasive change; (d) **mmap** the module files, for large codebases only (ours are kilobytes). Not applicable: jump tables / computed goto (a one-byte tag in a `match` already compiles to one), character classification and SIMD scanning (there is no text to scan).
  - **Resolved references (step 4), not built: a decision first.** Storing references by qualified name (or hash) means the stored tree is the *resolved* one, and the text's short names are derived when printing (from the module's `use`s). That changes what round-trips: `orders.Line` written in full where `Line` is imported would print back as `Line`. The resolver also rewrites structure (a module path `a.b.f` is field accesses before resolution), so it isn't a renaming of the same tree. Options: store resolved trees and print short names (text is canonicalised), or keep written names and add a per-definition table from written name to qualified name (cheap, but the checker still resolves). Ties in with the hash store above.
  - still to do: make the binary the source of truth (`edit` / `rename` / `move` / `done` writing it, the log and verdicts alongside or inside), which needs the other points below settled;
  - how it relates to the hash store above: a binary file per module, or one object per definition under its hash plus a names table;
  - review on GitHub: binary files show no diff in a PR. Locally a git `textconv` driver (`strata render`) and the `merge-file` driver can work on trees, but GitHub's PR view can't; `strata diff` or the viewer would be where changes are reviewed;
  - what `ci`'s canonical-format check becomes (the binary is canonical by construction);
- **Verification fast enough to run with every check** (user, 2026-11-20; direction, not designed). If agents wait on verification it isn't worth having: the goal is a few milliseconds for a short function, so verifying can run alongside type checking. Measured on `examples/tasks` (67 definitions): `check` 10 ms; `verify` 13 s, of which 108 solver runs take 11 s. Two queries take 6.2 s and 2.8 s (they run into the solver's limits); the others take about 15 ms each, mostly starting a `z3` process. Fuzzing and the cross-check are most of the remaining 2 s. Verdicts are already cached by content hash in a package, so an edit only re-verifies what changed and what depends on it. Ways to get there:
  - keep the solver running: one `z3` / `cvc5` process in incremental mode (`push` / `pop`), or link Z3 as a library, instead of a process per query;
  - two tiers: a fast pass next to the type check with a small budget per query, where anything that doesn't finish is reported as still being checked; and a slow pass in the background (full limits, fuzzing, the cross-check);
  - a verification server, like a language server: it keeps the syntax tree, the checker's environment, the call graph and the verdicts in memory, re-verifies on each change, and answers commands, the viewer and agents (through the Strata MCP server). `strata ui` is already a server; this pairs with storing the tree in binary (load once) and the Strata IDE;
  - make the cache as modular as the proofs. A proof of a caller already uses each callee's contract, not its body. But cached results are keyed by `closure_hash`, the code of everything a function reaches, so editing a callee's body throws away every caller's result. Key them instead by the callee's interface, except where the body does matter: helpers the prover sees through (`transparent`), termination results, and the cross-check, which runs the callees;
  - run independent queries in parallel.
  To decide: what an agent sees while the slow pass runs, and whether `edit` waits for the fast pass.
- **Strata IDE**: the viewer as a standalone desktop app (Tauri; the backend is already Rust), speaking ACP (Agent Client Protocol) so agents such as Claude Code or Codex run inside it, with a Strata MCP server giving them structural tools (slices, check, edit, rename). The viewer would show live where each agent is working (log entries tagged with the agent's session). Idea stage.

## Ideas

### Language

#### Ranges with Rust's `..` syntax (2026-11-20)

Write ranges as Rust does, instead of calling `range(lo, hi)`:

- `0..n` is `0, 1, …, n - 1` (end excluded), and `1..=n` includes `n`;
- `for i in 0..n { … }`, and `xs.map(…)` style chains on a range;
- open ends where they make sense: `xs[2..]`, `xs[..3]` (this meets Python-style slicing, under open questions).

It pairs well with list comprehensions (below): `[x * x for x in 1..=10 where x % 2 == 0]`. For the verifier, a range is already exact (every element is an `Int` with `lo <= x < hi`), so `0..10` gives `List[Digit]` for free.

To decide: whether `range(lo, hi)` stays, whether a range is a `List[Int]` or a lazy value of its own (iterators are a separate language feature), and steps (`(0..n).step(2)` or `0..n by 2`).

#### Default values for record fields (2026-11-20)

A record's definition can give a field a default, written explicitly where the
field is declared. Nothing gets a default implicitly.

```rust
type Options = {
  /// Say it this many times.
  times: Int where 1 <= it && it <= 10 = 1,
  /// Shout the greeting.
  loud: Bool = false,
  names: List[String] = [],
}

let o = Options { names: ["Ada"] }   // times: 1, loud: false
```

Questions to settle:
- **Syntax**: `field: T = value` looks natural. Check that it doesn't clash with
  a refinement that ends in a comparison.
- **What a default can be**: only literals and constructors, or any expression
  of the earlier fields? A default must satisfy the field's refinement, and
  the checker can prove that at definition time.
- **Where defaults apply**:
  - constructors (a missing field takes its default);
  - JSON input (a missing key takes its default; today it's an error);
  - command-line options (`CommandLine` would read `times: Int = 1` as an
    optional `--times` without needing `Option`).
- **Generated values**: mocks and fuzzing still generate every field.

#### List comprehensions in the style of refinements (2026-11-20)

Reuse the refinement syntax (`where`, and maybe `it`) to filter and map lists,
instead of chaining `.filter(...).map(...)`. Possible shapes:

```rust
xs where it > 0                      // filter: the elements that satisfy the condition
[x * x for x in xs where x > 0]      // map + filter, with a named element
```

Questions to settle:
- Whether `xs where …` on a value reads too much like a type, given that
  `Int where …` already means a refinement type.
- Several generators (`for x in xs, y in ys`), and dict and set forms.
- **Laziness**: comprehensions would stay eager for now. Lazy evaluation (and
  so infinite lists) is not wanted as a general mechanism. If it comes, it
  comes with iterators, which are already planned as their own language
  feature. A comprehension over an iterator could then be lazy.
- The prover could model `xs where p` directly (every element satisfies `p`,
  and the length is at most `len(xs)`), which `filter` with a lambda can't
  easily give it.

### Web UIs (direction agreed with the user 2026-11-20; not built)

The second place Strata should be useful, after the command line: UIs in the
browser, written without JavaScript. The direction is the Elm architecture,
run on the server, in the way Phoenix LiveView and htmx work:

```rust
use std.web.{App, Html, div, button, text, on_click}

type Model = { count: Int where 0 <= it }

type Msg = | Increment | Decrement | Reset

impl App[Msg] for Model {
  fn update(mut self, msg: Msg) -> () {
    match msg {
      Increment => self.count = self.count + 1
      Decrement => if self.count > 0 { self.count = self.count - 1 }
      Reset     => self.count = 0
    }
  }

  fn view(self) -> Html[Msg] {
    div([], [
      button([on_click(Decrement)], [text("-")]),
      text(f"{self.count}"),
      button([on_click(Increment)], [text("+")]),
    ])
  }
}
```

(The syntax is a sketch; traits with a type parameter like `App[Msg]` may
need work.)

`strata serve` keeps each session's `Model` in memory and renders `view` to
HTML. A small, fixed JS file that ships with the runtime catches clicks and
form submits. `on_click(Increment)` puts the message's JSON in an attribute;
the JS posts it back, and the server reads it against `Msg` (refinements
checked, as for command-line options), runs `update` and `view`, and the JS
swaps the new HTML into the page.

Why it fits: users write no JS; no async is needed in user code; `update` and
`view` are pure, so they can be tested without a browser and the verifier can
prove invariants across every message (`count >= 0`); an unhandled `Msg` is a
non-exhaustive `match`. The interpreter is plain Rust (the solver isn't needed
to run code), so it could later be compiled to wasm and run the same `App` in
the browser, without changing user code.

Steps:
1. `std.web.Html` (**built**, branch `worktree-web-html`): `Html[M]` (element,
   text) and `Attr[M]` (attribute, flag, event), helpers (`div`, `a`,
   `input`, …, `class`, `href`, `on_click`, …), `render` and `document`.
   Text and attribute values are escaped; names are checked where they are
   written (`element(tag: TagName, …)`, `attr(name: AttrName, …)`; `AttrName`
   excludes `on…` so inline scripts can't be written), not in the type:
   `Html[M] = { node: Node[M] }` and `Attr[M]` wrap private sums, so only
   `std.web` makes them and the verifier has nothing to check in views
   (decided with the user: verifying markup is pointless; what matters is
   the Strata values put into it, which JSX will make explicit); events are `data-on-EVENT` attributes holding the message as JSON
   (new builtin `to_json`). Void elements can't have children (`element`'s
   `requires`). CSS goes in `style_sheet(css)` (not escaped, can't close the
   element) or `style_link(url)`. Everything in `std` is proven, and so is
   `examples/page`. While `Html`'s fields were refined, the prover needed two
   changes in `smt.rs`, kept since they help any recursive or generic type:
   `check_type` stops at the third nested level of the same named type (a
   recursive type like `Html` made it re-check every element of every list,
   level after level: minutes for a small page), with one unknown per (type,
   value) so an assumed result and a later check are the same atom; and
   validity is compared on types whose unrefined type arguments are erased
   (`erased`), so a generic helper's result `Html[M]` matches its use as
   `Html[Msg]`. Not yet: `javascript:` URLs in `href` are not rejected.
2. `strata serve`: a low-level handler `fn handle(req: Request) -> Response`
   for JSON APIs and static files; bodies and query strings read into records
   like `CommandLine`. The HTTP layer is native, next to the `host_*`
   functions (`ui.rs` already has a std-only server).
3. `App` and the JS client: sessions, messages, swapping the HTML in.
4. Forms: `on_submit(constructor)` fills a record from the fields, with
   refinement errors shown next to each field.
5. Later: capabilities in `update` (database, LLM calls, HTTP client); the
   wasm build.

**JSX-like syntax** (user: wanted at some point, for simplicity). Markup
written directly in Strata expressions, compiled to the `Html` helpers, e.g.
`<button on_click={Increment}>-</button>`. Helper functions come first; the
syntax is added on top of them later. The parser checks the static markup (tag and
attribute names, no `on…` attributes, no children in void elements) as
syntax; only the `{…}` insertions are Strata code, and their types do the
rest: a `String` becomes escaped text, an `Html[M]` a child, an `M` an event's
message. The verifier has something to say only where an insertion's type is
refined (e.g. `href={url}` with a `Url` that excludes `javascript:`).

Questions to settle:
- **Async first, or web first?** (user, 2026-11-20; undecided.) Async is still
  planned (roadmap item 4). The web design above doesn't need it in user code:
  the runtime handles concurrency, and each request is a plain call. Where it
  matters is `update` calling something slow (an LLM, an HTTP request): it
  would block the session, and with a sequential server, everything. Elm
  answers this without async: `update` returns *commands* (data describing
  what to do), the runtime performs them and feeds the results back as
  messages. Commands are values, so they're easy to test, mock and verify.
  Claude's view: build the web first, with commands, and design async after;
  the risk is that async later changes how commands are written. If async
  comes first, a command could simply be an async call.
- Can `view` use capabilities? Pure is simpler and checkable.
- Send the whole page on every message, or a diff? The whole page to start.
- Sessions: kept in memory to start; later stored through the ORM.

## Design principles (decided)

- **No effects.** Capabilities (database, queue, console, clock) are traits; code takes an implementation as a parameter or holds it in a field. `mock T` generates an implementation that respects the trait's contracts. Async and iterators will be designed later as their own language features, **not** as effects.
- **Immutable by default, mutable when declared**: `let mut`, field assignment, `mut` parameters and `mut self` with write-back to the caller's variable. Procedural code is normal, and there are no shared references.
- **Refinements are structural**; `type X = Y` with no `where` is an error, so use `distinct type` for a nominal type.
- **Modules meet only through their public (`pub`) contracts.** A folder of `.strata` files loads as modules, and `render --split` writes them back.
- **No backward compatibility.** Breaking changes are expected: never add shims so that older stored trees (`.strata/` objects) or old syntax keep working. Re-import from source instead.
- **Rendered code wraps at about 80 columns** by breaking lines, not soft-wrapping.
- **Viewer**: two levels (interface/source), the code view sliced around the selection, a type map as a separate view, and no Flow view.

## Decisions

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

**Built (step 1 and most of 2)**: `src/smt.rs` and `verify::check_fn`. See docs/verification.md. Results: `Proven { checks }`, `Fuzzed { cases, discarded, proven, unproven }` (each unproven check with its reason), `Counterexample { .., found_by: solver|fuzzer }` (a solver model is replayed in the interpreter first, and counts only if it really fails), `Incomplete`. Z3 through SMT-LIB text over a pipe (`z3 -in`, `rlimit` for determinism, solver version in the cache key); `verify --smt` prints the queries (termination ones too), `--checks` lists proven checks. Cross-check: proven functions that terminate are fuzzed a little (`--cross-check`, step budget per run; past it, a run proves nothing); a failure there is reported as a prover bug. Policies: `[verify]` in `strata.toml` (`proven`, `terminates`, `no_recursion`, each a list of modules) and `--require`. Calls already use the callee's contract (requires checked, ensures assumed), and the same call with the same arguments gets the same result (memo), so refinements re-evaluated in assumptions and goals line up. Impl methods are proven against their trait's contract; laws are still fuzzed only. Solvers: `smt::solver()` returns a `Solver { bin, version, kind }`. `kind` is `Cvc5` when `--version` mentions cvc5, else `Z3` (so a fake solver in the tests is run like Z3). `Solver::args` is `-in -T:20 rlimit=20000000` for Z3 and `--lang smt2 --produce-models --rlimit-per=5000000 --tlimit-per=20000` for cvc5. 5M cvc5 units is about 4 to 10 s of hard nonlinear arithmetic; typical queries use thousands. cvc5 1.4.2 needs no `--strings-exp` for `str.replace_all` / `str.to_int`. Its model is one line, with negatives as `(- 6)` and a backslash as `\u{5c}`. `parse_values` / `parse_str` read both formats (unit test `values`). The whole suite passes with `STRATA_SOLVER=cvc5`, and no test had to change. The examples get the same verdicts with both solvers; cvc5 is about 2x slower on `examples/tasks`. There is no portfolio yet (both solvers in parallel, first answer wins). cvc5 decides large Unicode regex classes exactly and quickly, where Z3 times out, so with cvc5 those classes need not be approximated (see the Unicode-regex change).
Unions (`Val::Union`): a tag plus one slot per member *shape* (`val_key` / `type_key`: `Int`, `rec:Square`, `sum:Shape`, …; members of the same shape share a slot, e.g. `Positive | Negative` or two variants of one sum, whose inner tag is then constrained). Since shapes are disjoint the tag is a function of the value, as in the interpreter where a union value is just the member's value. Validity is `in_type` (= the interpreter's `value_in`: `shape_test` and `check_type`); merging values of different shapes makes a union; a `match` arm whose pattern can only match one slot rebinds the matched variable to that slot (exact under the arm's guard); a union used as an `Int`/`String`/`Bool` is `ite(tag = slot, member, fresh)`. Assigning a field of a sum or union value now makes it unknown (and the check unprovable) instead of keeping the old value. String builtins with an exact SMT-LIB counterpart are modelled (`slice` as `str.substr` with clamped bounds, `get`/`index_of` as `Option`s, `replace` as `str.replace_all` plus a not-empty-pattern check, `to_string` with a `-` for negatives, `ends_with`, `chars`, `is_digit`; `parse_int` exactly only for `-?[0-9]+` and digit-free strings, with the lemma `str.to_int x >= 0 <=> x in [0-9]+` that Z3 needs). `slice`, `get`, `take` and `drop` now accept big integers (saturated, so clamping is exact) instead of faulting. Z3 often gives up on `str.replace_all` with symbolic strings (those checks are fuzzed). Left unknown on purpose: `to_upper`/`to_lower`/`is_alpha` (Unicode tables), `ord`/`chr` (they expose code points, and Z3's strings stop at U+2FFFF), `trim` (Unicode whitespace at both ends), `split`, `lines`, `repeat`, `reverse`. Regexes (decided with the user: Strata regexes are modern Unicode regexes): `regex_model` parses with `regex-syntax` exactly as `regex::Regex::new` does (`^(?:re)$`, every option set explicitly) and translates the HIR (`hir_to_smt`); look-arounds only as `^`/`$` certain to be at the start/end; anything else is `None` (unknown). Z3 times out on unions of more than ~50-100 `re.range`s (cvc5 does not), so a class of more than `MAX_RANGES` (24) ranges makes `matches` a fresh bool between an under- and an over-approximation (`RegexModel::Between`); `regex_relations` adds exact inclusion/disjointness facts between patterns matched on the same string term, from a product of `regex-automata` dense DFAs (bounded search; undecided means no fact). SMT strings stop at U+2FFFF (`MAX_CHAR`): classes are clipped there, and literals past it are unknown. Before, `str_lit` wrote them as `\u{e0041}`, which Z3 reads as 9 ASCII characters, so `len("\u{E0041}") == 9` was "proven" (the cross-check caught it). Proofs about strings hold for strings up to U+2FFFF; the cross-check is the backstop. The fuzzer's generator (`rand_regex`) still can't generate from `\p{..}` or `\b`, so a function that uses them and isn't proven is reported as skipped.
Not yet: `assumed` (no outside code exists yet); loop invariants (loops forget what they change); list contents are "every element satisfies P" facts (`Val::List { len, id }`; `Vc::lists[id]` has a `Gen`, how an element is made, and guarded preds; `every` proves a property of an arbitrary generated element with the quantifier on the skolem side, `all_of` / `know_all` give `all(xs, p)` both directions; elements are made under their existence condition, so an empty literal can't make a path contradictory). Not modelled: positions and order (`xs[0]`, sortedness, permutations), relations between elements (uniqueness: `unique_ids`), `any`, `zip` / `enumerate`, dict and set contents; sequences (SMT `Seq`) could be added later for positions; floats, dicts, sets; `sum` is modelled only as "≥ 0 for non-negative elements, 0 when empty", `all`/`any` only when empty; generic callees lose precision (a result `MemQueue[P, E]` doesn't satisfy a `MemQueue[Int, String]` check, because element types are compared syntactically); unions whose members can't be told apart by shape (`List[Int] | List[String]`, `Box[Int] | Box[String]`, a type parameter, a trait, `Float`, `Dict`/`Set`, a refined sub-union) are unknowns.

### Termination and helpers (decided with the user, session 3; built)
- Termination is reported for every function: terminates (with how) / may not terminate (with why) / assumed (`terminates` marker, after the contract). See docs/verification.md. The viewer shows "may not terminate" in orange; diffs show it in the verification word.
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

### A type's own methods, and shared method names (decided 2026-11-20, built)
- **`impl Item { ... }`** gives a type methods of its own, with no trait. A method with `self` is called on a value; one without `self` is called on the type (`Item.new(...)`), and is resolved into the function `Item.new` right after module resolution.
- **Traits as parameter types** are shorthand for a type parameter with that bound (`mut queue: Queue[P, E]` = `[Q: Queue[P, E]](mut queue: Q)`). Name the parameter only to tie types together.
- **Method names may be shared between traits**, each implementation with its own body (revised 2026-11-20 by the user). A call through a trait (`d: Describe`, `T: Describe`) runs that trait's implementation. On a known type, the type's own method wins, else the one trait of the type that declares the name. Anything else is a compile error: a type with two such traits and no own method; a receiver known only as `A | B` or `T: A + B` (even if the actual type has its own method: that doesn't say which trait's meaning the call wants).
- **Elaboration** (`types::elaborate`): the checker records each such decision by call node, and the program that runs (interpreter, verify, SMT, termination) has it written in: `x.Describe::describe()` for a call through a trait, a direct call of the function `Item.describe` for a type's own method. A type's own methods are registered as functions (`Item.new`, `Item.describe`).
- **Trait methods without `self`** (built 2026-11-20): called on a named type (`Money.empty()`, `T.empty()`, `Self.empty()` in default methods and laws), elaborated to `Trait::empty(type, ..)`. The signature must mention `Self`. Generic functions whose type parameters are bounded by such a trait get hidden parameters `type:T` (of type `$Type[T]`, the type's name at runtime); each call passes the inferred type or forwards the caller's. The fuzzer generates them from its chosen instance; laws and trait methods know `Self` from the interpreter's `self_types` stack.

## Git, diffs and review (decided)

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

## Packages, modules and dependencies (decided)

- **A package is a folder with a `strata.toml`**: `name` (required; `strata init` writes the folder's name, and it stays when the folder moves, e.g. in a worktree), `exports` (optional, modules or single definitions, the equivalent of `pub use`; default every `pub`), `[deps]`, `[verify]` (verifier policy, e.g. modules that must be `proven`). There is **no `lib.strata`**. **`main.strata`** is the root module and the default entry point. A `.strata` file outside every package is a script. A nested folder with its own `strata.toml` is a separate package.
  **Built:** `name`, `exports` (checked by `ci`), discovery, nested packages, `init`. `[deps]` and `[verify]` are parsed but unused.
- **A file is a module; `module m { }` blocks are gone** (built). The parser has no `module` form, `printer::program` is one file's text, and `render` prints one module (`render MODULE`, `main` for the root) or `--split DIR`; on several modules without either it says so. A view across modules labels each group with a `// ── module m` comment line. Where a definition sits inside a file doesn't matter, and the standard rendering is one file per module. Folders organise modules freely; there are no submodules with special access. Still to confirm: whether a module's name is its path inside the package (`shop/orders.strata` → `shop.orders`, as today; recommended, so names don't collide and moving a file is a module rename) or the file name alone.
- **Dependencies from any git URL** (Go-like, not only GitHub), declared in `[deps]` (`json = "https://…"`, or `{ path = "../x" }` in the same repo), used as `use json.parse`. Fetched into a global cache and run directly, with no install step. Proposed, not yet confirmed: a generated `strata.lock` pinning each dep to a commit + content hash, and `strata update`. Semver tags (explained to the user) can come later; Strata could compute the right bump from interface hashes and contract diffs. Capabilities-as-values mean a dependency can't do I/O unless handed a capability, which makes URL imports much safer than npm/Go.

## Standard library: I/O first (built)

The user asked for a standard library usable from any program, starting with I/O (print, exit codes), then strings, regex, a NumPy-like math library (later, bigger), and web UI. Built so far:
- **`std/` is a Strata package** (`name = "std"`), embedded in the binary (`src/stdlib.rs`, `include_str!`). A program that has a `use std.…` gets std's definitions appended after its own at resolution (`stdlib::with_std` in `resolve::resolve`), so index `i` of a resolved program is still the program's `i`; code that zips resolved with original decls is unaffected, and `store::graph` truncates to the program's own. Its modules are `std.io`, `std.fs`, `std.process`, also when `std/` itself is loaded (`package::program` prefixes a package named `std`; `store::module_path` maps back to files).
- **Capabilities, as decided**: `Console`, `Files`, `Environment` are traits. The real implementations (`Terminal`, `Disk`, `Process`) are private std types, built only by `run_main` in `main.rs`. Test stand-ins are public: `Recorder`, `MemFiles`, `FakeEnvironment`.
- **The native layer** is the `host_*` builtins (`stdlib::HOST`, implemented in `Interp::host_call`). The resolver rejects them outside `std` modules. They work only when `Interp.host` is set, which only `strata run` does, so test/verify/fuzzing can never print, read or delete anything.
- **`strata run [PATH] [-- ARGS]`** runs `main`: parameters are filled by type (Console/Files/Environment only), the return is `()` → 0, `ExitCode`/`Int` → that code, `Result` → `Ok` as before or `Err` printed to stderr → 1; a fault → 101.
- Examples: `examples/hello`, `examples/wc` (with tests that use the stand-ins).
- Also fixed: the printer now escapes tabs and backslashes in string literals (it used to write a literal tab, and `"\\n"` came back as a newline).

### `print` and command-line options (decided with the user 2026-11-20; built, branch `worktree-stdlib-print-cli`, stacked on `worktree-stdlib-io`)
- **Capabilities stay** (user, asked explicitly): I/O is reached through `Console` / `Files` / `Environment` values; the value is the prefix (`files.read…`, `console.write…`). Plain ambient `fs.read(path)` functions were considered and rejected.
- **Except `print`**, which is a prelude builtin, available with no `use`: shorthand for "the console of wherever the code runs". `Interp.output`: `Terminal` under `strata run` (both `main` and `run <fn>`), `Buffer` per test under `strata test` (printed under the failure only), `Discard` everywhere else (verify, fuzzing, static checks). A plain call `print(x)` always means the builtin, even where an imported trait has a `print` method (`resolve::PRELUDE_FNS`); `console.print(x)` is the method. A failing `assert` no longer prints a second time when it re-evaluates its sides for the message.
- **`CommandLine`** (`std.cli`) is a marker trait with no methods (user's choice over "any record in main"). A `main` parameter whose type implements it is read from the arguments by `src/cli.rs`: `Bool` → flag, `Option[T]` → `--name value` / `--name=value`, `List[T]` → remaining positionals (one at most), anything else → required positional in field order. `T` = Int / Float / String / a refinement of one / a choice (variants without data, written in kebab case). Values are built as JSON and checked with `Interp::from_json`, so refinements are enforced; errors name the option and exit 2; `--help` comes from `///` docs on the record and its fields. `strata check` rejects a `CommandLine` record with fields a command line can't fill.
- **Fields can have `///` doc comments** now (`Field.doc`; parsed, printed, ignored by hashes like other docs).
- Empty trait and impl bodies print as `{}`.
- **Done (branch `worktree-stdlib-files-names`, stacked on #16):** `Files` methods are `read`, `write`, `append`, `exists`, `list`, `make_dir`, `remove`. Two general rules made that possible (`Checker::by_receiver`, recorded as `Target::Other` / `Target::Builtin` and written by `elaborate`):
  - a method name is matched by its short name against the traits of the value it is called on, whatever module they come from: method names are qualified by module at resolution, and a scope maps `write` to only one of `std.io.write` / `std.fs.write` (the last import), so `console.write(s)` used to resolve to `Files.write`. Same rules as #13 otherwise (one candidate: it; several: the existing errors);
  - a trait method named like a builtin doesn't hide the builtin: on an Int/Float/Bool/String/List/Dict/Set whose type has no trait declaring the name, `d.remove(k)` / `remove(d, k)` is the builtin.
  - Also: `types::test_instance` never picks `stdlib::HOST_TYPES` (Terminal, Disk, Process) for a type parameter. The cross-check from #10 had fuzzed `fn f[C: Console]` with the real `Terminal` (the host gate stopped it, but it was reported as a prover bug).
- Still to do: module aliases `use std.fs as fs` (user: yes, but lower priority since the capability value already prefixes calls). Not designed yet: defaults for options other than `None`/`false`/`[]`, short flags (`-v`), subcommands (probably a sum type of records).

Done: std's real `Terminal` / `Disk` / `Process` impls are verdict `assumed` (`Verdict::Assumed`, `≈`; `--require proven` accepts them and lists them; the viewer shows them neutrally). Not yet / next: marking other code `assumed` (FFI, dependencies); `main` can't ask for a clock, random numbers, network or a process spawner; an `Err` from `main` is printed as JSON; verification cache keys don't include std's hash (a std change only comes with a new binary). Next modules the user listed: strings (beyond the builtins), regex, math/linear algebra (NumPy-like), web UI (HTML/CSS generation and serving, per the batteries-included plan below).

## Viewer (built)

Doc comments are shown as the description in the right panel (and stripped from the code, `strip_doc` in `ui.rs`); the inspector has no source block; code items are plain code with a margin marker; bodies open inline (`has_body` in the snapshot; impls and traits now have a signatures-only level 1 in `view.rs`); the header has *Code* / *Type map* tabs and the code toolbar has *expand all*. The selected definition starts open.

Still open: **type map as a side panel** instead of a full tab. The user suggested it; ask before doing it.

Relations (decided): a definition has **dependencies** (what it mentions) and **references** (what mentions it). Implementing a trait is a separate relation, never a reference or a dependency: an `impl T for X` block is listed under *Implements* on `X` and *Implementations* on `T`, and its own trait and type are shown as *Trait* / *For*. The snapshot carries `deps`, `refs`, `impls` and `impl_of`. The type map shows dependency edges by default; *implementations* and *refinement types* are opt-in. There are no effects and no functions in the type map. The user wondered about a traits-only view, showing how traits work together; not decided.

## Known gaps

- No explicit type arguments at a call (`f[Money]()`): a type parameter that only a method called on the type needs (not in the parameters or result) can't be given. Generic impls' own type parameters can't have methods called on them yet (`impl[T: Empty] Empty for Pair[T]` calling `T.empty()`); only functions get hidden type arguments.
- No general FFI: the only native code is the standard library's `host_*` layer, so a real SQLite backend isn't possible yet.
- `strata verify --force` on the tasks example takes a few seconds (proven functions are not fuzzed); the slowest part is still generating `MemQueue` values that satisfy its invariants.
- `!` converts errors only by fitting (a variant into its sum, a member into a union), not with a conversion function.

## Known limitations

- **Proofs cover integers, strings, records, sums, unions and list lengths.** Loops forget what they change (no invariants yet), list elements are tracked only when the whole list is known to be valid, floats, dicts and sets are not modelled, and trait laws are fuzzed only.
- Refinements on generic type arguments, such as `Option[Quantity]`, are not checked when values are constructed.
- Length conditions on a list are only decided as "empty" or "not empty" (`where len(it) == 1` with `where len(it) >= 2` needs a `[]` or `_` arm).
- **No compilation backend.** It is a tree-walking interpreter.

## Roadmap

1. **SMT verification, continued**: loop invariants (inferred where possible, written where it helps), list contents, trait laws, and `assumed` for outside code beyond the standard library (FFI, dependencies).
2. **Traits with laws**: e.g. `trait Monoid { … law assoc(a, b, c): … }`; laws are verified or fuzzed for every implementation.
3. **Agent-friendly CLI everywhere**: `--json` on every command, trace queries, and scoped workspaces and leases so several agents can work on different slices at once. The CLI *is* the agent API; no server is needed.
4. **Async and iterators** as language features (whether before or after web UIs is open, see "Web UIs" under Ideas); `mock record` / `mock replay` for integration tests; deterministic replay debugging.
5. **Viewer**: show which agent is working on which slice, and add a diff view for each patch.
6. **Structural records with row polymorphism**: "anything with `.id: OrderId`".
7. **A compilation backend** (WasmGC or Cranelift).
