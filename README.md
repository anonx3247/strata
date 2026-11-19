# Strata

An experimental programming language designed for working with agents.

- **Code is a database, not files.** Definitions are stored as syntax trees, addressed by content hash. Text is a *view*, rendered at whatever level of detail you need, and edits are parsed back into the tree.
- **Every change is checked and atomic.** An edit is committed only if the *whole* codebase still type-checks. The log records whether each change touched an interface or only an implementation, and which definitions depend on it.
- **Incomplete programs still check.** Typed holes (`?name`) have a goal type, a context and contract obligations, so a sketch made of signatures and contracts can be verified before it is implemented, and each hole is a precise task to hand to an agent.
- **Constraints live in the types.** Refinement types (`Int where 1 <= it && it <= 99`, `String where matches(it, "...")`), `requires` / `ensures` contracts, exhaustive pattern matching, and algebraic data types.
- **Capabilities are values.** A database, clock or console is a trait (`trait Db { ... }`). Code that needs one takes it as a parameter or carries it in a field, so the signature shows what the code can touch. A test passes a fake implementation, or `mock Db`, which answers with generated values that satisfy the trait's contracts.
- **Everything is JSON.** Every value serializes to JSON automatically. JSON inputs are parsed against the types, with every refinement validated and errors pointing at the exact path.

> Status: **first experimental draft** (~4.3k lines of Rust). Contracts are checked at runtime and by
> property-based fuzzing, not yet *proven*. See [the roadmap](#roadmap).

## Try it

```sh
cargo build --release
alias strata=$PWD/target/release/strata

# Work directly on a file…
strata --file examples/orders.strata check
strata --file examples/orders.strata test
strata --file examples/orders.strata verify

# …or in a codebase database
mkdir /tmp/shop && cd /tmp/shop
strata init
strata add path/to/examples/orders.strata
strata view --level 0              # map of everything
strata view slice:Order --level 1  # Order, what it uses, what uses it: interfaces only
strata holes                       # what's left to implement
strata ui                          # open http://127.0.0.1:7777: the live viewer
```

## The viewer

`strata ui` serves a local, read-only viewer. It re-reads the codebase every 1.5 seconds, so edits made through the CLI (by you or by agents) appear as they are committed.

- **Code**: with a definition selected (the default once you click something), it shows that definition, then what it uses, what uses it, its implementations and its tests, as plain code with a small kind/status marker in the margin. Each definition shows its interface; where there is more (a function body, an implementation's methods, a trait's default methods), an inline *show body ▸* / *hide body ▾* control reveals it. The selected definition starts open, and *expand all* opens everything. *Everything* shows the whole codebase.
- **Hover a name** in the code to get an arrow to where that definition is on screen (or which way to scroll); click it to go there.
- **Inspector** (right): the selected definition's name and description (its `///` doc comment, which is not repeated in the code), a small graph of its neighbours (what it uses on the left, what uses it on the right, all clickable), its holes and verification result with any counterexample, and its history.
- **Type map**: a graph of types and traits and how they relate: *implements* (a type points at every trait it implements, so it is a graph, not a hierarchy), *extends* (supertraits), *contains* (a field or variant mentions it), *refines*. Each module is a region. The layout puts the most connected definitions at the centre of their module. Clicking a card shows it in the inspector; double-click opens it in the code view. Drag cards (positions are remembered), scroll to zoom, or *re-layout*.
- **Activity**: the patch log. Definitions changed since your last look are highlighted.
- The URL keeps the view, e.g. `#level=1&sel=refund`, so you can send someone or an agent exactly what you're looking at.
- Keys: `c` code, `t` type map, `1` collapse all, `2` expand all, `/` filters, `Esc` clears the selection.

`strata ui --file some.strata` views a single file instead of a codebase.

## A taste

`examples/bank.strata` is the smallest complete example. `examples/orders.strata` is the larger one excerpted below. `examples/tasks/` is a port of a real library, with a write-up of what verification found.

```rust
type OrderId  = String where matches(it, "ord_[a-z0-9]{6}")
type Quantity = Int where 1 <= it && it <= 99
type Cents    = Int where it >= 0
type NonEmpty[T] = List[T] where len(it) > 0

type Line = { sku: String, qty: Quantity, unit_price: Cents }

type Order =
  | Pending   { id: OrderId, lines: NonEmpty[Line] }
  | Shipped   { id: OrderId, lines: NonEmpty[Line], tracking: String }
  | Cancelled { id: OrderId, reason: String }

trait Db {
  fn load(self, id: OrderId) -> Option[Order]
  fn save(mut self, o: Order) -> ()        // `mut self`: saving changes the store
}

fn ship[D: Db](mut db: D, id: OrderId, tracking: String) -> Result[Order, ShipError]
  ensures match it { Ok(o) => o is Shipped, Err(_) => true }
{
  match db.load(id) {
    None => Err(NotFound),
    Some(Pending { id, lines }) => {
      let o = Shipped { id, lines, tracking }
      db.save(o)
      Ok(o)
    }
    Some(_) => Err(NotPending),
  }
}

fn refund(o: Order) -> Cents
  ensures it >= 0
  = ?refund_policy          // a typed hole: callers still check

test "a mocked store never breaks ship's contract" {
  let mut db = mock Db
  let r = ship(db, "ord_zzz999", "TRK9")
  assert r is Ok || r is Err
}
```

## What the tools tell you

```text
$ strata check                               # static errors are caught before running
error in status: non-exhaustive match on `Order`: `Cancelled { .. }` is not covered
error in sneaky: performs `Db.load` but `Db` is neither declared by `sneaky` nor handled here
error in bad_qty: refinement violated at compile time in field `Line.qty`: 120 does not satisfy `Quantity` (where 1 <= it && it <= 99)

$ strata holes
?refund_policy  in refund
  goal:    Cents  (= Int + refinement)
  context: o: Order
  must:    ensures it >= 0

$ strata verify                              # fuzz every contract with generated inputs (and mocks of traits)
✓ ship                     200 cases, 0 discarded by `requires`
✗ refund                   Refinement violation
    input:   [{"Shipped":{"id":"ord_1t6kdl","lines":[{"sku":"vbehwh","qty":993,"unit_price":0}],"tracking":"27c7p"}}]
    result of `refund`: -500 does not satisfy `Cents` (where it >= 0) (stack: refund)

$ strata edit -m "steeper discount" < discount.strata
fn     discount                 changed  #fc40aad7… → #2e54e58d…  (implementation only — dependents unaffected)

$ strata edit < breaking_change.strata
error in test "totals": call to `total`, argument 1: expected `Int`, found `Line`
rejected: the codebase would not check; nothing was changed

$ strata rename total order_total
renamed total → order_total
updated 1 definition that refers to it: test "totals"

$ strata run ship '["ord_abc123","TRK1"]' --mock --trace
{"event":"call","fn":"ship","args":["ord_abc123","TRK1"],"depth":1}
{"event":"perform","op":"Db.load","args":["ord_abc123"],"result":{"Some":{"Cancelled":{"id":"ord_so7f76","reason":"ju_4p-"}}}}
{"event":"return","fn":"ship","value":{"Err":"NotPending"},"depth":1}
```

## Refinements: named, inline, and structural

Refinements are structural: `type Quantity = Int where 1 <= it && it <= 99` is an `Int` plus a check, not a new nominal type. Any `Int` can be passed where a `Quantity` is expected, with no cast, and the check happens where the value crosses the boundary: at compile time for literals, at runtime otherwise, and under `verify`. A constraint used in one place doesn't need a name at all:

```rust
fn pick(level: Int where 5 <= it && it <= 13) -> Positive { level - 4 }
fn halve(n: Natural) -> Int where it >= 0 && it * 2 <= n { n / 2 }   // results may mention parameters
```

The prelude provides the common ones: `Natural`, `Positive`, `Negative`, `NonZero`, `Probability`, `Percentage`, `NonEmpty[T]`, `NonEmptyString`. `strata builtins` lists them with every builtin function.

A bare alias adds nothing, so `type Name = String` is an error. Constrain it with `where`, or make it a separate nominal type:

```rust
distinct type UserId = String where matches(it, "u_[0-9]+")
let id = UserId("u_42")      // explicit construction, checked
id.value                     // the underlying String
```

A `distinct` type never mixes with its base or with other distinct types (`OrderId` ≠ `UserId`), it can have trait implementations, and it serialises to JSON as its underlying value.

## Traits

A trait describes what a *type* can do. The implementation is chosen by the type of the value. A type can implement any number of traits, so they form a graph rather than a hierarchy.

```rust
trait Describe { fn describe(self) -> String  ensures len(it) > 0 }
trait Summary: Describe + Priced {                       // supertraits
  fn summary(self) -> String { self.describe() + ": " + to_string(self.price()) + "c" }   // default method
}
trait Combine {
  fn combine(self, other: Self) -> Self
  law associative(a: Self, b: Self, c: Self): a.combine(b).combine(c) == a.combine(b.combine(c))
}
impl Combine for Money { ... }
impl[T] Combine for List[T] { ... }                      // generic impls; built-in types work too
fn combine_all[T: Combine](first: T, rest: List[T]) -> T { fold(rest, first, (acc, x) -> acc.combine(x)) }
```

- **Contracts** (`requires` / `ensures`) on a trait method bind every implementation.
- **Laws** are checked by `strata verify` against every implementation, with generated inputs. In `examples/traits.strata`, `verify` finds that averaging (`Midpoint`) is not associative.
- **Generic functions** are fuzzed with a type that actually implements their bounds.

**Capabilities are traits too.** There are no separate effects: a backend (database, queue, console, clock) is a trait, and code that uses one takes an implementation as a parameter or holds it in a field: `fn ship[D: Db](db: D, ...)`, `type PrintWorker = { console: Console }`. A trait can be used as a type, meaning "any implementation". A method that changes its value says `mut self` (`fn save(mut self, o: Order)`). Tests pass a fake implementation or `mock Db`. Async and iterators will be designed as their own language features.

**Not yet:** methods without `self` (such as `fn empty() -> Self`), which need dispatch on the expected type; method names shared between traits.

## Modules: units that meet only through contracts

```rust
module orders {
  pub type Order = { id: OrderId, lines: NonEmpty[Line] }
  pub fn ship(id: OrderId) -> Result[Order, ShipError] / {Db}  ensures ...
  fn helper() -> Int { ... }            // private: invisible outside `orders`
}
module billing {
  use orders.{Order, ship}              // or `use orders.*`, or write `orders.ship(x)`
  pub fn invoice(o: Order) -> Cents { ... }
}
```

- A module is used **only through its public contract**: `pub` types, function signatures, `requires`/`ensures`, trait laws. Anything else is private, and using it from another module is an error ("`secret` is private to module `a`").
- `strata interface orders` prints that contract, with a hash. Editing a private definition, or a function body without touching its signature, is reported as "implementation only": nothing outside the module can be affected. Changing a `pub` signature or type says so ("module core: its public contract changed").
- **Files or database, your choice.** `--file` and `add` accept a folder, where each `.strata` file is a module named after its path (`shop/orders.strata` → `shop.orders`, `main.strata` → the root module). `render --split DIR` writes the codebase back out one file per module, and it round-trips. In the database, definitions are stored under qualified names, so `orders.Line` and `billing.Line` can coexist. `edit --module m` targets a module, and `view module:m` shows one.
- The viewer groups the sidebar by module (private definitions in italics) and lays the type map out as one region per module, with the modules others depend on placed first.
- `examples/tasks/` is now four modules (`core`, `memory`, `print`, `sim`) that use each other only through their contracts.

## Mutation: immutable by default, mutable when you say so

```rust
fn total(lines: List[Line]) -> Natural {
  let mut sum = 0
  for line in lines { sum = sum + line.qty * line.price }
  sum
}
fn restock(mut line: Line, extra: Int) -> () { line.qty = line.qty + extra }   // changes the caller's variable
```

Procedural code is normal Strata: `loop { … }` repeats until `break`, and `continue` skips to the next iteration. Method chains can be split across lines:

```rust
let best = scores
  .filter(s -> s > 0)
  .sort()
  .reverse()
  .take(3)
```

 Variables are immutable unless declared `let mut`. `while` and `for` loops are available, and fields can be assigned (`line.qty = …`). Mutation is always *visible*:
- a function that changes an argument declares it `mut`, and the caller must pass a mutable variable, which is updated when the call returns;
- a method that changes its receiver takes `mut self`.

There are no shared references, so nothing changes behind your back. Refinements still hold after every assignment: `line.qty = 120` fails if `qty` is a `Quantity`. Closures cannot change outer variables.

## Built-in types

| Type | Notes |
|---|---|
| `Int`, `Float`, `Bool`, `String` | `Float` is never NaN or infinite; there is no implicit Int/Float mixing (`float(n)`, `round(x)`) |
| `()` | the unit type: "no meaningful value", Python's `None` as a *type* |
| `Option[T]` | `None` or `Some(x)`: a value that may be absent |
| `Result[T, E]` | `Ok(x)` or `Err(e)`: an operation that may fail |
| `List[T]` | an ordered sequence, `[1, 2, 3]` |
| `(A, B, …)` | tuples, `(word, 3)`, destructured with `let (w, n) = …`, accessed with `t.0` |
| `Dict[K, V]` | `dict([("a", 1)])`, `get`, `insert`, `remove`, `keys`, `values`, `items`; JSON object when keys are strings |
| `Set[T]` | `set([1, 2])`, `insert`, `union`, `intersection`, `difference`, `to_list` |

All values are immutable (`insert` returns a new dict). Dicts and sets are kept in canonical order, so equal collections are equal values and serialise identically. `len`, `contains`, `get`, `insert`, `remove`, `sum`, `min`, `max` and `abs` work across the types they make sense for. `strata builtins` lists every builtin with its signature. See `examples/builtins.strata`.

**Not yet:** lazy iterators, generators and async, which will be designed as language features of their own.

## Views (levels of detail)

| Level | Shows |
|---|---|
| `--level 0` | a map: one line per definition, with its dependencies, holes and verification status |
| `--level 1` | interfaces only: types, traits, signatures and contracts |
| `--level 2` | an outline: L1 plus a one-line summary of each body |
| `--level 3` | full source; `render` output parses back to the same tree |

Comments are part of the tree, so they survive every edit and render. A `//` comment belongs to what follows it: a definition (comments separated from it by a blank line, such as a file header or a section banner, keep that blank line), a field, a variant, a trait or impl method, a statement or a match arm. A comment at the end of a line moves above the thing it follows. A comment anywhere else (inside an argument list, after the last statement of a block) is an error, never silently dropped. `///` doc comments work the same way and are the description the viewer shows. Comments are printed as written; changing one changes the definition's hash but not its interface hash, and does not invalidate verification results.

All rendered code is formatted to about 80 columns: long argument lists, record literals, method chains, `&&` / `||` / `+` chains, parameter lists and doc comments are broken across lines.

Selectors: `name`, `prefix*`, `slice:Name` (Name, what it uses, and what uses it), or a comma-separated combination.
Every view is Strata text, so an agent can edit what it sees and send it back with `strata edit`.

## How it is stored

```text
.strata/
  index.json          name → hash, in order
  objects/<hash>.json the syntax tree of one definition (content-addressed)
  log.jsonl           one entry per commit: which definitions changed, interface vs. implementation, which dependents are affected
  verdicts.json       verification results, keyed by the hash of a definition *and everything it depends on*
```

Because results are keyed by content, `verify` reruns only what is affected by a change.

## Source layout

| File | What it is |
|---|---|
| `src/ast.rs` | the syntax tree (the canonical representation) |
| `src/lexer.rs`, `src/parser.rs` | text → tree |
| `src/printer.rs` | tree → text (canonical form, used for hashing) |
| `src/types.rs` | type inference, traits, exhaustiveness, holes |
| `src/interp.rs` | interpreter: trait dispatch, mocks, runtime contracts, JSON in/out, traces |
| `src/generate.rs` | value generators derived from types and refinements (used by mocks and fuzzing) |
| `src/verify.rs` | compile-time refinement checks on literals; contract fuzzing |
| `src/store.rs` | content-addressed codebase, dependency graph, patch log |
| `src/view.rs` | level-of-detail projections and selectors |
| `src/ui.rs`, `src/ui/index.html` | `strata ui`: the live viewer (a std-only HTTP server and one page) |

## Known limitations of this draft

- **Verification is fuzzing plus runtime checks, not proof.** Next step: an SMT backend (Z3 or cvc5) that proves what it can and tags the rest as `fuzzed` or `runtime-checked`.
- Refinements on generic type arguments, such as `Option[Quantity]`, are not checked when values are constructed.
- **No compilation backend.** It is a tree-walking interpreter.

## Roadmap

1. **SMT-backed refinements**: obligations proven automatically where possible; counterexamples from the solver.
2. **Traits with laws**: e.g. `trait Monoid { … law assoc(a, b, c): … }`; laws are verified or fuzzed for every implementation.
3. **Agent-friendly CLI everywhere**: `--json` on every command, trace queries, and scoped workspaces and leases so several agents can work on different slices at once. The CLI *is* the agent API; no server is needed.
4. **Async and iterators** as language features; `mock record` / `mock replay` for integration tests; deterministic replay debugging.
5. **Viewer**: show which agent is working on which slice, and add a diff view for each patch.
6. **Structural records with row polymorphism**: "anything with `.id: OrderId`".
7. **A compilation backend** (WasmGC or Cranelift).
