# Tools

The command line, the viewer, and how a codebase is stored and merged.

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
    input:   [99]
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

$ strata run total '[[{"sku":"tea","qty":2,"unit_price":450},{"sku":"cup","qty":1,"unit_price":1200}]]' --trace
{"event":"call","fn":"total","args":[[{"sku":"tea","qty":2,"unit_price":450},{"sku":"cup","qty":1,"unit_price":1200}]],"depth":1}
{"event":"call","fn":"line_total","args":[{"sku":"tea","qty":2,"unit_price":450}],"depth":2}
{"event":"return","fn":"line_total","value":900,"depth":2}
{"event":"call","fn":"line_total","args":[{"sku":"cup","qty":1,"unit_price":1200}],"depth":2}
{"event":"return","fn":"line_total","value":1200,"depth":2}
{"event":"return","fn":"total","value":2100,"depth":1}
2100

$ strata run total '[[]]'                    # arguments are checked against the types
error: invalid input: lines: [] does not satisfy `NonEmpty` (where len(it) > 0)
```

## The viewer

![The Strata viewer, showing collatz_steps from examples/math.strata: its code, six proven checks, and "may not terminate"](images/viewer.png)

*`strata ui` on [`examples/math.strata`](../examples/math.strata) after `strata verify`. Left: every definition, with a dot for its status. Middle: the selected function and what surrounds it. Right: its description, neighbours, and what the verifier said: six checks proven, termination not shown.*

`strata ui` serves a local, read-only viewer. It re-reads the codebase every 1.5 seconds, so edits made through the CLI (by you or by agents) appear as they are committed.

- **Code**: with a definition selected (the default once you click something), it shows that definition, then how it takes part in implementations (the impl blocks of a type or a trait; an impl block's trait and type), its dependencies, its references and its tests, as plain code with a small kind/status marker in the margin. Each definition shows its interface; where there is more (a function body, an implementation's methods, a trait's default methods), an inline *show body ▸* / *hide body ▾* control reveals it. The selected definition starts open, and *expand all* opens everything. *Everything* shows the whole codebase.
- **Hover a name** in the code to get an arrow to where that definition is on screen (or which way to scroll); click it to go there.
- **Inspector** (right): the selected definition's name and description (its `///` doc comment, which is not repeated in the code), the traits it implements (or, for a trait, the types implementing it), a small graph of its neighbours (dependencies on the left, references on the right, all clickable), its holes and verification result with any counterexample, and its history.
- **Type map**: a graph of types and traits and how they depend on each other: *contains* (a field or variant mentions it), *refines*, *in a trait method* (a trait's method signatures mention it) and *extends* (supertraits). *implementations* adds the edges from each type to the traits it implements; *refinement types* adds types such as `type TaskId = Int where it > 0`. Both are off by default. Each module is a region. The layout puts the most connected definitions at the centre of their module. Clicking a card shows it in the inspector; double-click opens it in the code view. Drag cards (positions are remembered), scroll to zoom, or *re-layout*.
- **Activity**: the patch log. Definitions changed since your last look are highlighted.
- **Changes** (key `d`): a structural diff. On a codebase it covers patches from the log (by default everything after the first import; pick another start), one step per patch. On files (`--file`) under git it compares a revision (default `HEAD`, or a branch or commit you type) with the working tree, one step per commit plus uncommitted changes. Click a step, or use ← →, to see only that step.
  Each changed definition says what happened in words (a variant added, a method added, a stronger guarantee, renamed) and shows its code with the changes marked in place: unchanged members and lines fold away, keeping the lines that enclose each change; a new `ensures` is bold with a *stronger guarantee* pill (a removed one: *weaker*, a new `requires`: *stricter precondition*); a mostly rewritten body falls back to a line diff; a doc change is a word diff. A rename is one change, and definitions that only follow it are folded into it. Changes are ordered types → interfaces → implementations → tests, then verification before → after. The inspector shows what uses the selected definition, the steps that made the change, and its verification. `strata diff` prints the same thing as text (or `--json`).
- The URL keeps the view, e.g. `#level=1&sel=refund`, so you can send someone or an agent exactly what you're looking at.
- Keys: `⌘P` / `Ctrl+P` opens a palette to jump to any definition by name (fuzzy, recent ones first), `c` code, `d` changes, `t` type map, `1` collapse all, `2` expand all, `/` filters, `Esc` clears the selection, `←` / `→` go back and forward through the definitions you have visited (the toolbar has the same buttons; in *Changes* they walk through the steps).

`strata ui --file some.strata` views a single file instead of a codebase.

## Views (levels of detail)

| Level | Shows |
|---|---|
| `--level 0` | a map: one line per definition, with its dependencies, holes and verification status |
| `--level 1` | interfaces only: types, traits, signatures and contracts |
| `--level 2` | an outline: L1 plus a one-line summary of each body |
| `--level 3` | full source; `render` output parses back to the same tree |

Comments are part of the tree, so they survive every edit and render. A `//` comment belongs to what follows it: a definition (comments separated from it by a blank line, such as a file header or a section banner, keep that blank line), a field, a variant, a trait or impl method, a law, a statement or a match arm. Blank lines between statements are kept too (several in a row become one). A comment at the end of a line moves above the thing it follows. A comment anywhere else (inside an argument list, after the last statement of a block) is an error, never silently dropped. `///` doc comments work the same way and are the description the viewer shows. Comments are printed as written; changing one changes the definition's hash but not its interface hash, and does not invalidate verification results.

All rendered code is formatted to about 80 columns: long argument lists, record literals, method chains, `&&` / `||` / `+` chains, parameter lists and doc comments are broken across lines.

Selectors: `name`, `prefix*`, `slice:Name` (Name, what it uses, and what uses it), or a comma-separated combination.
Every view is Strata text, so an agent can edit what it sees and send it back with `strata edit`.

## How it is stored

```text
strata.toml           the package: name, exports, dependencies, verifier policy
main.strata, a/b.strata, …
                      one file per module, in canonical form (rewritten by `edit`, `add`, `rename`)
strata.log            one JSON entry per change made through strata: its id, message, which
                      definitions changed (with their text before and after), interface vs.
                      implementation, which dependents are affected, and renames
.strata/verdicts.json local cache (not committed): verification results, keyed by the hash of a
                      definition *and everything it depends on*
```

The files and `strata.log` are committed. The log is append-only and merges by union (`.gitattributes`), and each entry carries the definitions' text, so it replays without anything else: `strata diff` splits a git commit into the changes made through strata (with their renames), plus a step for edits made by hand. Because verification results are keyed by content, `verify` reruns only what is affected by a change.

A program can also be stored as its syntax tree, in binary: `strata pack DIR` writes one module file per module (`main.stb`, `shop/orders.stb`) beside the `strata.toml`, and `strata pack app.stb` writes the whole program as one bundle. `--file`, `strata run`, `strata ui --file` and every view read either like source; `strata unpack IN DIR` writes the source back out, one canonical file per module. Each module file starts with a header listing its definitions, their hashes and dependencies (`strata header core.stb` prints it), and each definition stores its interface before its bodies. The format is Strata's own, specified in [binary-format.md](binary-format.md). For now the text files stay the source of truth: `edit`, `rename` and `move` work on them, not on binary files. The hashes `strata` shows are those of the binary encoding (see the spec).

## Packages

A folder with a `strata.toml` is a **package**. Its `.strata` files are its modules, named after their path inside it (`shop/orders.strata` is `shop.orders`). `main.strata` is the root module and the default entry point; a library has none. A folder inside a package that has its own `strata.toml` is a separate package. A `.strata` file outside every package is a standalone script (like `examples/bank.strata`).

```toml
name = "tasks"                         # `strata init` uses the folder's name; it stays when the folder moves (a worktree, a rename)
exports = ["core", "memory.MemQueue"]  # optional: modules or definitions other packages can use; default every `pub`

[deps]                                 # accepted, not fetched yet
json = "https://example.com/strata-json"

[verify]                               # what `strata verify` requires (see Verification)
proven = ["core"]
```

`strata ci` rejects an export that names nothing, or a definition that isn't `pub`.

## Git: merging definition by definition

The `.strata` files are what you commit.

- **`strata merge-file %O %A %B %P`** is a git merge driver. It pairs each side's definitions with the base's. A definition that only one side changed comes from that side, so edits to two functions never conflict, even when they sit on neighbouring lines. When both sides changed the same definition, its canonical text is merged line by line, and the result is kept only if it parses back into that one definition; otherwise conflict markers go around that definition alone. A rename on one side (a definition gone, and a new one that is the base's after `strata rename`) is applied to the other side, so new calls to the old name follow it. Then the file is type-checked inside its package: an error that neither side had on its own (ours changed a signature, theirs added a call to the old one) fails the merge. Sides that don't parse are merged as text.
- **`strata setup-git`** points this clone's git config at the driver, and `.gitattributes` has `*.strata merge=strata`, so `git merge` and `git pull` use it.
- **Every change is a commit.** In a git repository, each change made through strata (an `edit`, `rename`, `move` or `add` that goes through, or a pending change at `strata done`) is committed with its message: `git log` is the history of changes, you can go back to any of them, and branches work as usual. Only the package's files are committed; whatever else is staged is left alone. Edits during a pending change aren't committed until it's done. Turn it off with `auto_commit = false` in `strata.toml`, or `--no-commit` on one command.
- **`strata sync [main]`** merges `origin/main` (or the local branch, without a remote) into the current branch with the driver, whether or not `setup-git` was run. Renames that either side made with `strata rename` (read from both sides' `strata.log`) are then applied to the other side's code, in every file: a call that main added to `area` becomes a call to `rect_area` if your branch renamed it, unless a local named `rect_area` would capture it. Then it runs `strata ci` and commits the merge only if that passes; otherwise the merge is left staged, so you can fix it or `git merge --abort`.
- **`strata ci [--fix]`** is what CI runs: every package and script in the repository checks, every package's exports exist, and every `.strata` file is in canonical format (exactly what `render` prints; `--fix` rewrites it). GitHub doesn't run merge drivers, so `.github/workflows/strata.yml` runs `strata ci` on the pull request's merge ref. Make it a required check, and require branches to be up to date before merging. Then a branch lands only once it was merged with `main` by `strata sync` and the result checks.
