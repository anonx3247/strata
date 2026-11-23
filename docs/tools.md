# Tools

The command line, and how a codebase is stored and merged.

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

$ strata edit < breaking_change.strata
error in test "totals": call to `total`, argument 1: expected `Int`, found `Line`
rejected: the codebase would not check; nothing was changed

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

$ strata edit total                       # nothing piped in: opens `total` in $VISUAL / $EDITOR;
                                          # what you save is the edit
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

## Compiling to native code

`strata build` compiles a program to an executable through LLVM (`clang` compiles the IR with a small runtime). Programs are compiled unoptimized (`-O0`), which is fastest to build; `--release` compiles at `-O2`. `build` first verifies what changed since it was last verified, and the checks proven are left out of the program (`--all-checks` keeps them; see [verification](verification.md#what-is-proven-isnt-checked-again)).

```text
$ strata build tests/data/build/numbers.strata -o numbers
built numbers
$ ./numbers
2432902008176640000
gcd = 6, 2^10 = 1024, A(2, 3) = 9
...
fault (Refinement): result of `next_digit`: 10 does not satisfy `Digit` (where 0 <= it && it <= 9)
```

A compiled program behaves as `strata run`: the same output, and contracts are checked at run time with the same fault messages, exiting with 101. A fault prints the call stack (`    stack: main -> inner -> half`). Memory is reference-counted: a value is freed as soon as nothing holds it.

`strata run` compiles the program in memory and runs it (a JIT), so it runs at native speed without a build step. `strata test` compiles the tests the same way: a fault ends the test, not the run, and what a test prints is shown only if it fails. How the compiler and the JIT work is in [internals](internals.md#compiling).

LLVM 23's libraries are linked into strata, so building it needs them (nothing is needed to run it):

```sh
brew install llvm                      # or apt.llvm.org's llvm-23-dev
LLVM_SYS_231_PREFIX=$(brew --prefix llvm) cargo build --release
```

## Serving the web

`strata serve [PATH] [--port N]` (default port 8000) runs a program as a web server: each request is turned into a `std.http.Request` and passed to the program's `fn handle(req: Request) -> Response`. Like `main`, `handle` may also ask for `Console` and capabilities (`Disk`, `Process`, the program's own). The program is compiled once, when the server starts (the JIT, as `strata run`; with a few functions of the server's, written in Strata and checked with the program), and requests are handled one at a time by that compiled code. A runtime fault, or a response the server can't send (a header holding a line break), becomes a 500 that says why, and every request is logged:

```text
$ strata serve examples/page
Serving on http://127.0.0.1:8000  (Ctrl-C to stop)
GET / 200 (17 ms)
GET /languages/haskell 200 (0 ms)
POST / 405 (0 ms)
```

**Apps.** A program with no `handle` but with `init`, `update` and `view` is served as an app (the Elm architecture): `fn init() -> Model`, `fn update(model: Model, msg: Msg) -> Model`, `fn view(model: Model) -> Html[Msg]`. Each page load starts a session whose model the server keeps; the page sends what the user does, one message at a time, and the new view is patched in, so focus and scroll stay. A page can only send the messages it offers. See `examples/counter`, and `examples/todos` for a form.

`handle` is a plain function, so tests call it directly (`handle(get_request("/"))`). See `examples/page` and, for routes, `std.http.Router` in [the language](language.md).

## Structural diffs

`strata diff` shows changes by definition rather than by line: each changed definition says what happened in words (a variant added, a method added, a stronger guarantee, renamed), and shows only what it needs. A rename is one line (`Order.Shipped → Order.Sent`); a new signature or contract shows the signature and its conditions, not the body; a changed body is shown with what didn't change folded away. Definitions that only follow an interface change (a rename, call sites given a new argument) are listed under it. Changes are ordered types → interfaces → implementations → tests, then verification before → after. `--json` gives the same for other tools.

## Views (levels of detail)

| Level | Shows |
|---|---|
| `--level 0` | a map: one line per definition, with its dependencies, holes and verification status |
| `--level 1` | interfaces only: types, traits, signatures and contracts |
| `--level 2` | full source; `render` output parses back to the same tree |

Comments are part of the tree, so they survive every edit and render. A `//` comment belongs to what follows it: a definition (comments separated from it by a blank line, such as a file header or a section banner, keep that blank line), a field, a variant, a trait or impl method, a law, a statement or a match arm. Blank lines between statements are kept too (several in a row become one). A comment at the end of a line moves above the thing it follows. A comment anywhere else (inside an argument list, after the last statement of a block) is an error, never silently dropped. `///` doc comments work the same way, and are what `--help` and the views show as a definition's description. Comments are printed as written; changing one changes the definition's hash but not its interface hash, and does not invalidate verification results.

All rendered code is formatted to about 80 columns: long argument lists, record literals, method chains, `&&` / `||` / `+` chains, parameter lists and doc comments are broken across lines.

Selectors: `name` (a definition, or a module with the modules inside it: `std.ast.check`), `prefix*`, `module:m`, `slice:Name` (Name, what it uses, and what uses it), or a comma-separated combination. A part that names nothing is an error.
Every view is Strata text, so an agent can edit what it sees and send it back with `strata edit`.

## How it is stored

```text
strata.toml           the package: name, exports, dependencies, verifier policy, C to link ([native]),
                      whether `main` may ask how it is run ([run] mode)
main.strata, a/b.strata, …
                      one file per module, in canonical form (rewritten by `edit`, `add`, `rename`)
strata.log            one JSON entry per change made through strata: its id, message, which
                      definitions changed (with their text before and after), interface vs.
                      implementation, which dependents are affected, and renames
.strata/proofs.json   local cache (not committed): what `verify` proved, for `build` to leave out
```

The files and `strata.log` are committed. The log is append-only and merges by union (`.gitattributes`), and each entry carries the definitions' text, so it replays without anything else: `strata diff` splits a git commit into the changes made through strata (with their renames), plus a step for edits made by hand. Because verification results are keyed by content, `verify` reruns only what is affected by a change, in a package, a script or `std/`, and in any worktree (`--no-cache` reruns everything).

The code is stored as its syntax tree, in binary: each `.strata` file is a module file (the format is Strata's own, specified in [binary-format.md](binary-format.md)). Text goes in only through an edit (`strata edit`, `strata add`) and comes out through the views (`render`, `view`).

## Packages

A folder with a `strata.toml` is a **package**. Its `.strata` files are its modules, named after their path inside it (`shop/orders.strata` is `shop.orders`). `main.strata` is the root module and the default entry point; a library has none. A folder inside a package that has its own `strata.toml` is a separate package. A `.strata` file outside every package is a standalone script (like `examples/bank.strata`).

```toml
name = "tasks"                         # `strata init` uses the folder's name; it stays when the folder moves (a worktree, a rename)
exports = ["core", "memory.MemQueue"]  # optional: modules or definitions other packages can use; default every `pub`

[deps]                                 # other packages, by path or git URL (read by the compiler in Strata, `compiler/deps`)
json = { path = "../json" }

[verify]                               # what `strata verify` requires (see Verification)
proven = ["core"]

[json]                                 # how `std.json` reads JSON here
unknown_keys = "error"                 # `from_json` refuses keys a record doesn't have (default: "ignore")
integers = "proven"                    # integer fields must be proven to fit a JSON number (default: "checked", an error from `to_json`)

[toml]                                 # how `std.toml`'s derived `Toml` reads TOML here
unknown_keys = "error"                 # `from_toml` refuses keys a record doesn't have (default: "ignore")
```

**Dependencies.** `[deps]` names other packages, by local path (`json = { path = "../json" }`) or git URL (pinned to a commit in `strata.lock`). A dependency's modules are named with its package's name as prefix, as std's are with `std` (its root module is `json`, its module `text` is `json.text`), and they are reached only through `use`, and only what its `exports` lists. There are no cycles between packages. So far dependencies are loaded by the compiler written in Strata only (`compiler/deps`).

## Git: merging definition by definition

The `.strata` files are what you commit. They are binary, so `.gitattributes` marks them `binary diff=strata merge=strata`: with `strata setup-git`, `git diff` and `git log -p` show their text, and git merges them with the driver. GitHub shows no diff for them.

- **`strata merge-file %O %A %B %P`** is a git merge driver. It pairs each side's definitions with the base's. A definition that only one side changed comes from that side, so edits to two functions never conflict, even when they sit on neighbouring lines. When both sides changed the same definition, its text is merged line by line; a conflict is marked around that definition alone. Renames on one side follow to the other, and the result is type-checked: an error that neither side had on its own fails the merge.
- **`strata setup-git`** points this clone's git config at the driver and at `strata render --file` to show `.strata` files as text, and `.gitattributes` has `*.strata binary merge=strata diff=strata`, so `git merge`, `git pull`, `git diff` and `git log -p` use them.
- **Every change is a commit.** In a git repository, each change made through strata (an `edit`, `rename`, `move` or `add` that goes through, or a pending change at `strata done`) is committed with its message: `git log` is the history of changes, you can go back to any of them, and branches work as usual. Only the package's files are committed; whatever else is staged is left alone. Edits during a pending change aren't committed until it's done. Turn it off with `auto_commit = false` in `strata.toml`, or `--no-commit` on one command.
- **`strata sync [main]`** merges `origin/main` (or the local branch, without a remote) into the current branch with the driver, whether or not `setup-git` was run. Renames that either side made with `strata rename` (read from both sides' `strata.log`) are then applied to the other side's code, in every file: a call that main added to `area` becomes a call to `rect_area` if your branch renamed it, unless a local named `rect_area` would capture it. Then it checks every package and script and commits the merge only if they check; otherwise the merge is left staged, so you can fix it or `git merge --abort`.
- **`strata upgrade`** rewrites every package and script in the repository (or the current folder outside git) in canonical form, exactly the bytes strata writes for it now, printing each file it rewrites: run it after a change to the tree or the format (see [the binary format](binary-format.md#changing-the-format)). GitHub doesn't run merge drivers, so merge `main` into a branch with `strata sync` before it lands.
