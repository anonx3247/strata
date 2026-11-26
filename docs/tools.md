# Tools

This page covers the command line, and how a codebase is stored and merged.

## Installing

On macOS (Apple silicon), the release includes LLVM, the runtime, std and Z3, so nothing else is needed:

```sh
curl -fsSL https://strata-lang.dev/install.sh | sh
```

It installs into `~/.strata` (`bin/strata` and `lib/`) and prints how to add `~/.strata/bin` to your PATH. Set `STRATA_VERSION=v0.1` to pin a release, `STRATA_INSTALL=DIR` to install somewhere else, or `STRATA_MODIFY_PATH=1` to add the line to your shell's rc file. Running the script again upgrades in place.

**For coding agents**, `strata skill install` writes a short skill, `strata/SKILL.md`, into the folders where agents look for skills: `~/.agents/skills` (Codex, Pi, OpenCode), plus `~/.claude/skills` and `~/.codex/skills` when those agents are installed. `--dir DIR` writes to one folder, such as a project's `.claude/skills`. The skill explains that `.strata` files are binary, that code is read and changed through `strata`, and lists the commands. `strata skill` prints it.

**Building from source** needs LLVM 23's libraries, zstd and clang, which are linked into strata and not needed to run it, and lld's libraries, built once into `target/lld`. Because strata is written in Strata, an existing `strata` builds the next one: `STRATA_SEED` if set, else one on the PATH, else the latest release, downloaded.

```sh
brew install llvm zstd                 # LLVM 23, and clang
tools/build-lld.sh                     # lld's libraries, into target/lld
tools/bootstrap.sh                     # target/strata, built by itself (a fixed point)
```

## What the tools tell you

```text
$ strata check                               # static errors are caught before running
error in status: non-exhaustive match on `Order`: `Cancelled` is not covered
error in sneaky: calling `load`: `String` does not implement `Db`
error in bad_qty: refinement violated at compile time in field `Line.qty`: 120 does not satisfy `Quantity` (where 1 <= it && it <= 99)

$ strata holes
?refund_policy  in refund
  goal:    Cents  (= Int + refinement)
  context: o: Order
  must:    ensures it >= 0

$ strata verify                              # prove every contract with Z3; fuzz what it can't prove
✓ line_total               proven (2 checks)
✓ total                    proven (3 checks)
✓ discount                 proven (2 checks)
✓ ship                     proven (5 checks)
○ refund                   incomplete: reached unfilled hole `?refund_policy` (via refund)

$ strata verify bump                         # fn bump(q: Quantity) -> Quantity { q + 1 }
✗ bump                     Refinement violation (found by the solver)
    input:   99
    result of `bump`: 100 does not satisfy `Quantity` (where 1 <= it && it <= 99) (stack: bump)

$ strata edit -m "steeper discount" < discount.strata
fn     discount                 changed  #fc40aad7… → #2e54e58d…  (implementation only — dependents unaffected)
checked 1 definition of 14: what the edit can affect

$ strata edit -m "steeper discount" < discount.strata    # the same again
no changes: every definition given is as stored (nothing written, checked or logged)

$ strata edit < broken.strata                # an edit that doesn't check in itself
error in total: `+` operands: expected `Int`, found `Bool`

rejected: the codebase would not check; nothing was changed
the text is kept in /…/shop/.strata/rejected/edit-1.st: fix it, then pipe it to `strata edit` again

$ strata rename total order_total
renamed total → order_total
updated 1 definition that refers to it: test "totals"

$ strata rename Shipped Sent              # a variant; trait methods work the same way
renamed Shipped → Sent
updated 3 definitions that refer to it: order_id, ship, test "shipping a pending order saves it as shipped"

$ strata rename Line.qty quantity         # a field: accesses, literals, updates, patterns
renamed Line.qty → Line.quantity
updated 3 definitions that refer to it: line_total, test "shipping a pending order saves it as shipped", test "totals"

$ strata rename A.tag label B.tag label   # several renames in one step (here, a field read from `A | B`)
renamed A.tag → A.label, B.tag → B.label
updated 1 definition that refers to them: tag_of

$ strata check                            # warnings don't fail it; `strata fix` makes their changes
warning: in `pay`, `requires price >= 100` only constrains `price`: that is a refinement of its type, `price: Cents where it >= 100` (`strata fix` makes the change)
ok: 12 definitions check (0 open holes)

$ strata fix                              # every warning's change, as one logged step (`strata fix pay`: one definition)
fixed: in `pay`, `requires price >= 100` only constrains `price`: that is a refinement of its type, `price: Cents where it >= 100`

$ strata move charge billing              # to another module (file): references and imports follow
moved orders.charge from orders to billing (billing.strata)
updated 1 definition that refers to it: test "charges"
imports: orders: use billing.charge; main: use billing.charge

$ echo 'fn tax(amount: Int, rate: Int) -> Int { amount * rate / 100 }' | strata edit -m "tax takes a rate"
the edit breaks 2 uses elsewhere; they are now holes, in a pending change:
  ?pending_1  in total
      was: tax(amount)
      why: call to `tax`: expects 2 arguments, got 1
  ?pending_2  in double_tax
      was: tax(a)
      why: call to `tax`: expects 2 arguments, got 1
fill them with `strata edit`, then `strata done` (or `strata abort` to undo the whole change)

$ strata done                             # once the holes are filled: one log entry for it all
recorded "tax takes a rate" as one change: 3 definitions

$ strata edit total                       # at a terminal, nothing piped in: opens `total` in
                                          # $VISUAL / $EDITOR; what you save is the edit
                                          # (with no one at the terminal, e.g. an agent's
                                          # shell: an error saying to pipe the definitions in,
                                          # or `--editor` to open the editor anyway)
$ strata edit shop                        # the module `shop` (or `shop.strata`, also a new module)
$ strata edit main.main                   # the definition `main` of the root module `main`
$ strata edit                             # the whole package, one file per module
$ strata edit shop < defs.strata          # piped: the definitions go into the module `shop`

$ strata run total '[Line { sku: "tea", qty: 2, unit_price: 450 }, Line { sku: "cup", qty: 1, unit_price: 1200 }]' --trace
→ total([Line { sku: "tea", qty: 2, unit_price: 450 }, Line { sku: "cup", qty: 1, unit_price: 1200 }])
  → line_total(Line { sku: "tea", qty: 2, unit_price: 450 })
  ← line_total: 900
  → line_total(Line { sku: "cup", qty: 1, unit_price: 1200 })
  ← line_total: 1200
← total: 2100
2100

$ strata run total '[]'                      # the arguments, as code, checked against the types
error: invalid input: lines: [] does not satisfy `NonEmpty` (where len(it) > 0)
```

**Copies.** `--copies` (on `run`, `test` and `build`) shows whether a loop is copying a value. When the program ends, it reports each copy made to change a value, grouped by place in the code (a location, for `strata view`), with the count, size and reason. With no copies, it prints `copies: none`. See [values and copying](language.md#values-and-copying) for when a value is copied.

```
$ strata run --copies
copies: 2 copies, 14 kB, at 1 place (the most bytes first):
  copied a Dict of up to 400 entries at app.kept@31f7e4cc/body/4/body/0/value: 2 times, 14 kB (shared: held 2 times)
```

**Timing.** macOS checks each new executable the first time it runs, so a built program's first run takes about a quarter of a second longer than later ones. Time the second run.

**Colour.** On a terminal, output is coloured: proven green, failures red, fuzzed or incomplete orange, a counterexample's input amber, hashes dim, holes yellow, function names bold, and in a diff removals red and additions green. Code is syntax-highlighted wherever it is shown (`view`, a counterexample's input, a hole's goal and context, a failed assertion), with the site's colours. Capabilities, and the traits they implement, have a colour of their own. As in programs that use `std.term`, colour is on when the output is a terminal, `NO_COLOR` is unset and `TERM` isn't `dumb`. `FORCE_COLOR` forces it on.

**Paging.** Output meant for reading (`view`, `find`, `refs`, `diff`, `log`, `holes`, `status`, `interface`, `builtins`, the help) goes through a pager on a terminal, as with git. The pager is `$STRATA_PAGER`, else `$PAGER`, else `less -FRX`. Unless `LESS` is set, it is run with `LESS=FRX`, which quits when the text fits on one screen and keeps colours. `--no-pager`, or `STRATA_PAGER=cat` or empty, turns paging off. `check`, `verify` and `test` stream their results, and `run` and `edit` never page.

## Compiling to native code

`strata build` compiles a program to an executable through LLVM, linked with a small runtime. Programs are compiled unoptimized (`-O0`), the fastest to build. `--release` compiles at `-O2`. `build` first verifies what changed since it was last verified, and leaves proven checks out of the program (`--all-checks` keeps them; see [verification](verification.md#what-is-proven-isnt-checked-again)).

```text
$ strata build tests/data/build/numbers.strata -o numbers
built numbers
$ ./numbers
2432902008176640000
gcd = 6, 2^10 = 1024, A(2, 3) = 9
...
fault (Refinement): result of `next_digit`: 10 does not satisfy `Digit` (where 0 <= it && it <= 9)
```

A compiled program behaves like `strata run`: the same output, and the same run-time contract checks and fault messages, exiting with 101. A fault prints the call stack (`    stack: main -> inner -> half`). Memory is reference-counted, and a value is freed as soon as nothing holds it.

`strata run` compiles the program in memory and runs it with a JIT, at native speed without a build step. If the program changed since it was last verified, it starts immediately while being verified alongside, and the proven code (without the proven checks) is swapped in while it runs ([verification](verification.md#what-is-proven-isnt-checked-again); see `--no-verify`, `--verify-alongside`, `--wait-for-proofs`). `strata test` compiles tests the same way. A fault ends the test, not the run, and what a test prints is shown only if it fails. [Internals](internals.md#compiling) describes how the compiler and the JIT work.

## Structural diffs

`strata diff` shows changes by definition rather than by line. This example follows four changes to `examples/orders.strata` made through strata: a variant renamed with `strata rename`, a stronger `ensures`, a new variant, and a body rewritten as a loop.

```text
$ strata diff
patches 2–5 of 5
  step 1 rename Shipped → Sent  (4 definitions)
  step 2 discount: never more than 10% off  (1 definition)
  step 3 a new ShipError  (1 definition)
  step 4 total as a loop  (1 definition)

5 changed  ·  3 interface changes

TYPES

~ Order  (variant `Shipped` renamed to `Sent`)
    only a name changed · 3 references updated
    type Order =
      | Pending { id: OrderId, lines: NonEmpty[Line] }
  ~   | [-Shipped-]{+Sent+} { id: OrderId, lines: NonEmpty[Line], tracking: NonEmptyString }
      | Cancelled { id: OrderId, reason: NonEmptyString }

~ ShipError  (variant `OutOfStock` added)
    interface changed · used by 2, 4 depend on it in all
    type ShipError =
    … NotFound and NotPending unchanged
  +   | OutOfStock { sku: NonEmptyString }

INTERFACES

~ discount  (guarantee changed)
    interface changed · used by 1
    fn discount(amount: Cents) -> Cents
  ~   ensures it <= amount {+&& it * 10 >= amount * 9 +}{    ◀ contract changed
      if amount >= 10000 {
    … 4 lines unchanged
    }

IMPLEMENTATIONS

~ total  (body changed)
    most of it changed, so it is shown as a line diff
    implementation only · nothing else needs re-checking
    fn total(lines: NonEmpty[Line]) -> Cents {
  -   sum(lines.map(line_total))
  +   mut sum := 0
  +   for l in lines {
  +     sum += line_total(l)
  +   }
  +   sum
    }

TESTS

~ test "shipping a pending order saves it as shipped"  (body changed)
    test "shipping a pending order saves it as shipped" {
    … 6 lines unchanged
  -   assert db.order == Some(
  ~ [-    Shipped-]{+  assert+} {+db.order == Some(Sent +}{ id: "ord_abc123", lines: [line], tracking: "TRK1" }[-,-]{+)+}
  -   )
    }
```

Each changed definition describes the change in words and shows only what is needed:

- A rename takes one line. `Order` shows the renamed variant. The three definitions that only followed the rename (`order_id`, `ship`, a test) are counted, not shown, unless their body also changed in another way, like the test below.
- A new variant is shown alone, with unchanged members folded (`… NotFound and NotPending unchanged`), and with how much depends on the type.
- A contract change is marked on its line (`◀ contract changed`). Here `discount` promises more, and its body is folded.
- A body change is shown with unchanged parts folded, or as a line diff when most of it changed (`total`). "implementation only" means nothing else needs re-checking.

Changes are ordered types, then interfaces, then implementations, then tests.

- `strata diff --step 2` shows one change. Steps are numbered from 1, as `diff` lists them, so `--from 1 --to 2` is patch 2.
- `--from` and `--to` select a range of the log, by the states between patches, where 0 is before the first.
- `--json` prints the same for other tools.
- In a git repository, `strata --file F diff [BASE]` compares a file with a commit (`HEAD` by default). After `strata setup-git`, `git diff` shows `.strata` files the same way.

## Views (levels of detail)

`strata view` is the one command that shows code.

- `strata view MODULE` (`shop`, `main` for the root module, or a file such as `shop/orders.strata`) prints a module as stored, imports first. This is the text `edit` reads back.
- `strata view SELECTOR --level N` shows the definitions a selector picks, at a level of detail (1 by default).
- `strata view` alone prints the package's map (level 0).
- `view --split DIR` writes every module as a text file (`shop/orders.st`) for reading, never for reading back.

| Level | Shows |
|---|---|
| `--level 0` | a map: one line per definition, with its dependencies, holes and verification status |
| `--level 1` | interfaces only: types, traits, signatures and contracts |
| `--level 2` | full source; `view MODULE` output parses back to the same tree |

**Comments** are part of the tree, so they survive every edit and view.

- A `//` comment belongs to what follows it: a definition, a field, a variant, a trait or impl method, a law, a statement or a match arm. Comments separated from a definition by a blank line, such as a file header or a section banner, keep that blank line.
- Blank lines between statements are kept, with several in a row collapsed into one.
- A comment at the end of a line moves above the thing it follows.
- A comment anywhere else (inside an argument list, after the last statement of a block) is an error. Comments are never silently dropped.
- `///` doc comments follow the same rules. `--help` and the views show them as a definition's description.
- Comments are printed as written. Changing one changes the definition's hash but not its interface hash, and doesn't invalidate verification results.

All rendered code is formatted to about 80 columns. Long argument lists, record literals, method chains, `&&`, `||` and `+` chains, parameter lists and doc comments are broken across lines.

**Selectors:** `name` (a definition, or a module with the modules inside it: `std.ast.check`), `prefix*`, `module:m`, `slice:Name` (Name, what it uses, and what uses it), or a comma-separated combination. A part that matches nothing is an error.

Every view is Strata text, so an agent can edit what it sees and send it back with `strata edit`.

## State graphs: how a type's values move

`strata view --graph T` derives a type's pipeline from the code; nothing declares it. For a sum type `T` (`Status = Pending | Running | Done | Exhausted`), it finds each function that takes a value holding a `T` and returns one. The `T` can be the value itself, a field at a path such as `Entry.status`, a list or dict of them, or a trait's model. The function can return it as its result, through a `mut` parameter, or through `mut self`. For each pair of variants, it asks whether the function can move a value from one to the other. A list's element is compared with the element at the same position, and a dict's value with the value at the same key.

```
$ strata --file examples/tasks view --graph Status
Status = Pending | Running | Done | Exhausted
  (new)     -> Pending    by MemQueue.enqueue (possible)
  (new)     -> Running    by MemQueue.claim (unknown)
  Pending   -> Running    by MemQueue.claim (possible)
  Pending   -> Done       by after_attempt (possible)
  Running   -> Pending    by MemQueue.enqueue (unknown), after_attempt (possible)
  ...
  Exhausted -> Running    by MemQueue.claim (unknown)
  Exhausted -> Done       by after_attempt (possible)
  any       -> any        by MemQueue.record (unknown), MemQueue.release (unknown), ... (nothing proven impossible: --all lists their moves)
proven impossible: 33 more (in 4 functions; --all lists them)
```

Each pass of `MemQueue.claim`'s loop puts only `Running` entries in its table, so the body's shape proves it makes no other move, and a run on generated inputs shows `Pending -> Running`. Some moves stay unknown: those that need a key followed through the loop (`Done -> Running`, since `claim` only takes pending entries), and those that need the trait's contracts over every entry of a dict (`all(d.items(), …)`, not modelled: `Queue.claim`, `run_worker`).

Each move is labelled with what is known about it:

- `possible`: the solver found values that make the move with an exact encoding of the function (no loop, no callee known only by its contract), or a run on generated inputs made it.
- `unknown`
- proven impossible: listed with `--all`.

The body's shape is checked first:

- A list kept as is (`filter`, slices, the first part of `xs + [x]`) moves nothing.
- An element added as written (`Entry { status: Pending, … }`) is an entry in that variant.
- A loop pass that inserts values written as specific variants (`mut self.table.insert(id, Entry { ..e, status: Running { … } })`) makes no move to any other variant.
- A method that hands its state to a function (`self = mem_claim(self, limit)`) moves as that function does.
- A trait method with models moves as its contract says (`ensures self.entries == …`).

A value created from nothing is an entry: `(new) -> Shipped by ship`. A function about which nothing is proven, because it calls functions whose contracts don't say what they do with the type, gets one `any -> any` line instead of a line per move. `--html` writes a page with the graph drawn: the variants in a row, each move an arc labelled with its functions and coloured by what is known.

**From a run.** `strata run --trace` prints each call and return. `strata view --graph T --trace FILE` reads those lines and counts the moves each call made (`Running -> Done by after_attempt ×2`).

**In a diff.** `strata diff --graph T` adds how the moves changed, in words (`new move `Running -> Cancelled` by `cancel``, `` `expire` no longer reaches `Pending` ``). `--html` draws both graphs side by side, with added and removed moves marked.

## Docs in the terminal: `strata doc`

`strata doc` shows the documentation in the terminal, paged and highlighted as `view` is. It works offline: a release carries these pages.

```text
$ strata doc                       # the pages, and how to look things up
$ strata doc language              # a page: docs/language.md
$ strata doc language#traits       # one of its sections, by its heading or a word of it
$ strata doc std.json              # a module's reference
$ strata doc Dict.insert           # a definition: signature, contracts, doc and cost
$ strata doc parse                 # a name defined in several modules lists them
```

A topic is looked up as a page first, then as a module, then as a definition. Definitions are searched in the program around the current folder first, then in core (the builtins), then in std. A builtin's doc ends with its cost.

The text is wrapped to the terminal's width, at most 100 columns, and to 80 columns when the output is not a terminal. Strata code is highlighted. Without color (`NO_COLOR`, or a pipe), the output is plain text.

## Reference pages: `strata docgen`

`strata docgen` writes a package's reference from its `///` doc comments: a Markdown page for each module with public definitions, and an index (`README.md`) listing them with the first sentence of each module's doc. `strata doc MODULE` shows the same page in the terminal.

- A page has the module's doc (the comment that opens its file), then its public types, their methods, its traits and its functions. Each is shown with its signature as Strata code, contracts and refinements included (as `view --level 1` prints it), and its doc. The implementations of its traits and for its types come last.
- `-o DIR` chooses the folder (default `doc`). `std.io` becomes `DIR/io.md`, and `shop.orders` becomes `DIR/shop/orders.md`.
- Naming modules writes only their pages.

```text
$ strata --file std docgen -o docs/std       # the standard library's reference, docs/std/
wrote 19 pages to docs/std
$ strata docgen --man -o man                 # man pages: man/man3/shop.orders.3
$ man -M man shop.orders
```

`--man` writes man pages instead, in section 3, named after the module (`std.io(3)`), in the folder `man` by default. For std, it also writes the `strata` command's page, `strata(1)`, generated from `strata help`. The [standard library's pages](std/README.md) on this site are generated by `strata docgen` into `docs/std/`; regenerate them after changing std's docs.

## Finding code: `find`, `refs`, locations

Three commands find code by what it is rather than by its text, so an agent doesn't need grep:

```text
$ strata find next
method    jobs.Worker.next(self) -> Option[Task]
$ strata find Task.attempts
field     jobs.Task.attempts: Int
$ strata refs Task.attempts --kind write
used by — 1 use in 1 definition

jobs.retried@5ce7a5fb/body/1/place  write
pub fn retried(t: Task) -> Task {
  mut u := t
  u.attempts = u.attempts + 1
  ^^^^^^^^^^
  u
}
$ strata view 'jobs.retried@5ce7a5fb/body/1/place'
```

- **Names** can be short (`run_worker`, in any module), qualified (`jobs.run_worker`), a glob (`'*verdict*'`), fuzzy (`~rnwrkr`), or a member (`Task.attempts`, `Worker.next`, `Shape.Circle`). A short name that matches several definitions shows them all: first the candidates with their number of uses, then each one's results.
- **`strata find NAME`** lists where NAME is defined, one line each: kind, qualified name and signature (`--doc` shows the doc's first line instead). Options: `--kind fn|type|trait|variant|field|method|impl|test|capability|extern`, `--in SELECTOR`, `--public`, `--limit N`. It reads only the module files' headers and interfaces, never bodies.
- **`strata refs NAME`** shows where NAME is used ("used by"). Each use is labelled with its kind (`call`, `construct`, `pattern`, `read`, `write`, `type`, `impl`, `import`, `method`) and shown as code in its definition: the head, the lines around each use (`--context N`, default 1, or `--context all`), the enclosing blocks, `⋮` for omitted lines, and `^^^` under the use. It then shows what NAME uses ("uses"). `--used-by` and `--uses` pick one direction, `--kind` filters kinds, `--in SELECTOR` limits where to look, and `--depth N` follows the graph and prints a tree. Field reads and method calls are resolved by the type the checker infers for the receiver, so another record's field with the same name isn't counted.
- **Locations.** Every result carries a location, `DEF@HASH/PATH`: the definition, the first 4 bytes of its hash, and a path of the tree's field names and list positions. `jobs.retried@5ce7a5fb/body/1/place` is the place assigned in statement 1 of the body. `strata view LOCATION` shows it as code (`--context N`, or `--def` for the whole definition). If the definition has changed since, it says so and shows the same path in the current version. `strata check` prints each error with its location and the code it refers to.
- `--list` prints locations only, and `--json` prints everything for scripts. The exit code follows grep: 0 if found, 1 if nothing, 2 for an error.

## How it is stored

```text
strata.toml           the package: name, exports, dependencies, verifier policy, C to link ([native]),
                      whether `main` may ask how it is run ([run] mode)
main.strata, a/b.strata, …
                      one file per module, in canonical form (written by `edit`, `add`, `rename`:
                      only the files that change; `strata upgrade` rewrites them all)
strata.log            one JSON entry per change made through strata: its id, message, which
                      definitions changed (with their text before and after), interface vs.
                      implementation, which dependents are affected, and renames
```

The module files and `strata.log` are committed. The log is append-only and merges by union (`.gitattributes`). Each entry carries the definitions' text, so the log replays on its own: `strata diff` splits a git commit into the changes made through strata, with their renames, plus one step for edits made by hand. Verification results are keyed by content, so `verify` reruns only what a change affects, in a package, a script or `std/`, and in any worktree (`--no-cache` reruns everything).

Code is stored as its syntax tree, in binary. Each `.strata` file is a module file, in Strata's own format, specified in [binary-format.md](binary-format.md). Text goes in only through an edit (`strata edit`, `strata add`) and comes out through `view`.

What `strata` compiles, proves and fetches is kept in a cache outside the package: `$STRATA_CACHE`, or else `~/.cache/strata`. Nothing in it is evicted automatically. `strata cache` shows where it is and the size of each folder. `strata cache clean` deletes it and reports how much space that freed; everything in it is rebuilt when needed.

## Packages

A folder with a `strata.toml` is a package. Its `.strata` files are its modules, named after their path inside it (`shop/orders.strata` is `shop.orders`). `main.strata` is the root module and the default entry point; a library has none. A folder inside a package with its own `strata.toml` is a separate package. A `.strata` file outside every package is a standalone script (like `examples/bank.strata`).

```toml
name = "tasks"                         # `strata init` uses the folder's name; it stays when the folder moves (a worktree, a rename)
exports = ["core", "memory.MemQueue"]  # optional: modules or definitions other packages can use; default every `pub`
noalloc = ["memory"]                   # optional: modules whose every function is `noalloc`, checked (and the modules below; `main`: the root module)

[deps]                                 # other packages, by path or git URL (`compiler/deps`)
json = { path = "../json" }

[verify]                               # what `strata verify` requires (see Verification)
proven = ["core"]

[json]                                 # how `std.json` reads JSON here
unknown_keys = "error"                 # `from_json` refuses keys a record doesn't have (default: "ignore")
integers = "proven"                    # integer fields must be proven to fit a JSON number (default: "checked", an error from `to_json`)

[toml]                                 # how `std.toml`'s derived `Toml` reads TOML here
unknown_keys = "error"                 # `from_toml` refuses keys a record doesn't have (default: "ignore")
```

**Dependencies.** `[deps]` names other packages by local path (`json = { path = "../json" }`) or by git URL, pinned to a commit in `strata.lock`. A dependency's modules are prefixed with its package name, as std's are with `std`: its root module is `json`, and its module `text` is `json.text`. They are reached only through `use`, and only for what its `exports` lists. Packages can't depend on each other in a cycle.

## Git: merging definition by definition

The `.strata` files are what you commit. They are binary, so `.gitattributes` marks them `binary diff=strata merge=strata`. After `strata setup-git`, `git diff` and `git log -p` show their text, and git merges them with the driver. GitHub shows no diff for them.

- **`strata merge-file %O %A %B %P`** is a git merge driver. It pairs each side's definitions with the base's. A definition that only one side changed comes from that side, so edits to two functions never conflict, even on neighbouring lines. When both sides changed the same definition, its text is merged line by line, and a conflict is marked around that definition alone. A rename on one side is applied to the other, and the result is type-checked: an error that neither side had alone fails the merge.
- **`strata setup-git`** configures this clone's git to use the driver, and to show `.strata` files as text with `strata view main --file`. `.gitattributes` has `*.strata binary merge=strata diff=strata`, so `git merge`, `git pull`, `git diff` and `git log -p` use them.
- **Every change is a commit.** In a git repository, each change made through strata (an `edit`, `rename`, `move` or `add` that succeeds, or a pending change at `strata done`) is committed with its message. `git log` is the history of changes, you can go back to any of them, and branches work as usual. Only the package's files are committed, and anything else staged is left alone. Edits during a pending change are committed only when it is done. Turn this off with `auto_commit = false` in `strata.toml`, or `--no-commit` on one command.
- **`strata sync [main]`** merges `origin/main` (or the local branch, without a remote) into the current branch with the driver, whether or not `setup-git` was run. Renames that either side made with `strata rename`, read from both sides' `strata.log`, are then applied to the other side's code in every file. A call that main added to `area` becomes a call to `rect_area` if your branch renamed it, unless a local named `rect_area` would capture it. It then checks every package and script, and commits the merge only if they all check. Otherwise the merge is left staged, so you can fix it or run `git merge --abort`.
- **`strata upgrade`** rewrites every package and script in the repository (or in the current folder, outside git) in canonical form, the exact bytes the current strata writes, printing each file it rewrites. Run it after a change to the tree or the format (see [the binary format](binary-format.md#changing-the-format)).

GitHub doesn't run merge drivers, so merge `main` into a branch with `strata sync` before it lands.
