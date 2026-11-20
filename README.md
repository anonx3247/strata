# Strata

An experimental programming language designed for working with agents.

- **You work on definitions, not files.** Definitions are syntax trees, identified by content hash. Text is a *view*, rendered at whatever level of detail you need, and edits are parsed back into the tree. The files (one per module, in canonical form) and a log of every change are what git stores.
- **Every change is checked and atomic.** An edit is committed only if the *whole* codebase still type-checks. The log records whether each change touched an interface or only an implementation, and which definitions depend on it.
- **Incomplete programs still check.** Typed holes (`?name`) have a goal type, a context and contract obligations, so a sketch made of signatures and contracts can be verified before it is implemented, and each hole is a precise task to hand to an agent.
- **Constraints live in the types.** Refinement types (`Int where 1 <= it && it <= 99`, `String where matches(it, "...")`), `requires` / `ensures` contracts, exhaustive pattern matching, and algebraic data types.
- **Capabilities are values.** A database, clock or console is a trait (`trait Db { ... }`). Code that needs one takes it as a parameter or carries it in a field, so the signature shows what the code can touch. A test passes a fake implementation, or `mock Db`, which answers with generated values that satisfy the trait's contracts.
- **Everything is JSON.** Every value serializes to JSON automatically. JSON inputs are parsed against the types, with every refinement validated and errors pointing at the exact path.

> Status: **first experimental draft**. Contracts are proven by an SMT solver (Z3) where it can, and
> what it can't prove is fuzzed. See [Verification](#verification) and [the roadmap](#roadmap).

## Try it

```sh
cargo build --release
alias strata=$PWD/target/release/strata

# Work directly on a file…
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
strata ui                          # open http://127.0.0.1:7777: the live viewer
```

## The viewer

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

## A taste

`examples/bank.strata` is the smallest complete example. `examples/orders.strata` is the larger one excerpted below. `examples/tasks/` is a port of a real library, with a write-up of what verification found.

```rust
type OrderId  = String where matches(it, "ord_[a-z0-9]{6}")
type Quantity = Int where 1 <= it && it <= 99
type Cents    = Int where it >= 0

type Line = { sku: NonEmptyString, qty: Quantity, unit_price: Cents }

type Order =
  | Pending   { id: OrderId, lines: NonEmpty[Line] }       // NonEmpty, NonEmptyString: from the prelude
  | Shipped   { id: OrderId, lines: NonEmpty[Line], tracking: NonEmptyString }
  | Cancelled { id: OrderId, reason: NonEmptyString }

trait Db {
  fn load(self, id: OrderId) -> Option[Order]
  fn save(mut self, o: Order) -> ()        // `mut self`: saving changes the store
}

fn ship(mut db: Db, id: OrderId, tracking: NonEmptyString) -> Result[Order, ShipError]
  ensures match it { Ok(o) => o is Shipped, Err(_) => true }
{
  let order = db.load(id) except { return Err(NotFound) }
  match order {
    Pending { id, lines } => {
      let o = Shipped { id, lines, tracking }
      db.save(o)
      Ok(o)
    }
    _ => Err(NotPending),
  }
}

fn refund(o: Order) -> Cents
  ensures it >= 0
  = ?refund_policy          // a typed hole: callers still check

test "a mocked store never breaks ship's contract" {
  let mut db = mock Db
  let r = ship(mut db, "ord_zzz999", "TRK9")
  assert r is Ok || r is Err
}
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

## Unions, variants and patterns

A union holds a value of any of its members. It is a union of sets, so it normalizes: `Int | Positive` is just `Int`, and different types stay apart.

```rust
type Square = { side: Positive }
type Circle = { radius: Positive }
type Shape = Square | Circle

fn area(s: Shape) -> Int {
  match s {
    Square => s.side * s.side,      // s: Square in this arm
    Circle => 3 * s.radius * s.radius,
  }
}
fn describe(v: Int | String) -> String {
  match v { Int => f"number {v}", String => f"text {len(v)}" }
}
```

**Variants are types.** `type ShapeError = | Empty | TooBig { max: Int }` declares the types `Empty` and `TooBig`, and `ShapeError` is `Empty | TooBig`. A function can take `e: TooBig` (and read `e.max`), or return `Result[Shape, TooBig]`. Constructors still need wrapping (`Err(TooBig { max: 9 })`), so `Result[Int, Int]` stays unambiguous. Since a variant is a type, its name must be unique across the project: no other variant, type or trait in any module may share it.

**Patterns are sets of values**, and an arm takes what the arms above it left:

- a type's name: `Positive =>`, `String =>`, `Circle =>` (a variant, whatever its fields), or a trait (`Named =>`, values that implement it);
- a variant or record with fields, as before: `Some(x)`, `Pending { id, .. }`;
- a literal: `0`, `"x"`, `true`;
- a condition on the value `it`: `where 3 <= it && it <= 5 =>`, `where len(it) > 0 =>`, `where it < 0.5 =>`;
- alternatives: `Square | Triangle =>`, `0 | 1 | 2 =>`, `(0, x) | (x, 0) =>` (they bind the same names);
- `_`.

**Narrowing.** Inside an arm, the matched variable has the arm's type (`s` is a `Square` above), and no new name is needed. To match the result of an expression, name it: `match lines(text) as ls { … }`. A matched field (`match t.status { Closed => t.status.why, … }`) narrows too. Assigning to a narrowed `let mut` variable ends its narrowing.

**Exhaustive and reachable.** The checker splits the matched type into variants, union members, and the ranges the arms' bounds cut out: of an `Int`, of a `Float`, or of the length of a `String` or `List`. It reports a value no arm takes (`` `where 3 <= it && it <= 5` is not covered``), and an arm the arms above already cover (`unreachable match arm `Positive``). Conditions that compare `it` (or `len(it)`) with constants, combined with `&&`, `||`, `!` and `==>`, are decided exactly. Any other condition (`matches(it, …)`, a call, another variable) covers nothing until the SMT backend decides it, so such a match needs a `_` arm.

**The matched value's own refinement counts.** In `fn f(n: Positive)`, `match n { 1 => …, where it > 1 => … }` is exhaustive, `match xs { [x, ..] => … }` is exhaustive for `xs: NonEmpty[Int]`, and a `where it <= 0` arm on `n` is unreachable (`no `Positive` matches it`). This holds for variables (mutable ones too: a declared type holds after every assignment), their fields, a function's result type, and inside a narrowing arm. Narrowing follows the code, though: an arm that assigns the matched `mut` variable (anywhere in the arm, so a loop's next pass is covered too) does not narrow it, and the variable keeps its declared type there.

A field that every member of a union has can be read without matching. A trait method can be called on a union when every member implements the trait. Alternatives of different types (`if` branches, `match` arms, list elements) get their union when no expected type says otherwise: `if big { Square {…} } else { Circle {…} }` is a `Square | Circle`.

**Type arguments fit member-wise.** A `List[Circle]` can be passed where a `List[Shape]` is wanted, because values are never shared: the callee works on its own copy. A function fits when it accepts more and returns less. The exception is a `mut` parameter, which writes back into the caller's variable: its argument needs exactly the parameter's type (`grow(mut shapes: List[Shape])` could put a `Square` into a `List[Circle]`). For the same reason, `!` passes an error into a wider error type (a `TooBig` into a `Result[_, ShapeError]`).

## Traits

A trait describes what a *type* can do. The implementation is chosen by the type of the value. A type can implement any number of traits, so they form a graph rather than a hierarchy.

```rust
trait Describe { fn describe(self) -> String  ensures len(it) > 0 }
trait Summary: Describe + Priced {                       // supertraits
  fn summary(self) -> String { f"{self.describe()}: {self.price()}c" }   // default method
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

**Capabilities are traits too.** There are no separate effects: a backend (database, queue, console, clock) is a trait, and code that uses one takes an implementation as a parameter or holds it in a field: `fn ship(mut db: Db, ...)`, `type PrintWorker = { console: Console }`. A trait used as a type means "any implementation": `db: Db` is shorthand for `fn ship[D: Db](db: D, ...)`. Name the type parameter only when it must tie things together, as in `combine_all[T: Combine](first: T, rest: List[T]) -> T`. A method that changes its value says `mut self` (`fn save(mut self, o: Order)`). Tests pass a fake implementation or `mock Db`. Async and iterators will be designed as their own language features.

**A type's own methods.** `impl Item { ... }` gives a type methods of its own, with no trait. A method with `self` is called on a value (`item.priced(42)`). One without `self` is called on the type, which makes it a constructor: `Item.new("tea")`. Only a record, sum or `distinct` type defined in the program can have them, in the module that defines it, and in a single `impl Item` block.

```rust
impl Item {
  fn new(name: NonEmptyString) -> Item { Item { name, cents: 0 } }
  fn priced(self, cents: Natural) -> Item { Item { ..self, cents } }
}
let tea = Item.new("tea").priced(42)
```

**Shared method names, and precedence.** Two traits may declare methods with the same name. A call picks the method by the type of the value it is called on, and **a type's own method always takes precedence**: it also serves every trait of the type that declares that name. So a type that implements two traits that both have `describe` must define `describe` in `impl Item`. That one method then serves both traits, the trait impls leave it out, and it is checked against both signatures and verified against both contracts. A name that depends on the receiver can't be passed as a bare function value (`map(describe)`): pass a lambda (`x -> x.describe()`).

**Not yet:** trait methods without `self` (such as `fn empty() -> Self` in a trait, called as `T.empty()` in generic code).

## Modules: units that meet only through contracts

A **file is a module**, named after its path in the package: `shop/orders.strata` is `shop.orders`, and `main.strata` is the root module. There are no `module { }` blocks.

```rust
// orders.strata
pub type Order = { id: OrderId, lines: NonEmpty[Line] }
pub fn ship(mut db: Db, id: OrderId) -> Result[Order, ShipError]  ensures ...
fn helper() -> Int { ... }            // private: invisible outside `orders`

// billing.strata
use orders.{Order, ship}              // or `use orders.*`, or write `orders.ship(x)`
pub fn invoice(o: Order) -> Cents { ... }
```

- A module is used **only through its public contract**: `pub` types, function signatures, `requires`/`ensures`, trait laws. Anything else is private, and using it from another module is an error ("`secret` is private to module `a`").
- `strata interface orders` prints that contract, with a hash. Editing a private definition, or a function body without touching its signature, is reported as "implementation only": nothing outside the module can be affected. Changing a `pub` signature or type says so ("module core: its public contract changed").
- **Files or database, your choice.** `--file` and `add` accept a folder, where each `.strata` file is a module named after its path (`shop/orders.strata` → `shop.orders`, `main.strata` → the root module). `render MODULE` prints one module (its file), and `render --split DIR` writes the codebase back out one file per module; both round-trip. A view that spans several modules (`view a,b`) labels each group with a `// ── module m` comment. In the database, definitions are stored under qualified names, so `orders.Line` and `billing.Line` can coexist. `edit --module m` targets a module, and `view module:m` shows one.
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
restock(mut line, 5)                                                         // and the call says so
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
- a function that changes an argument declares it `mut`, and the caller passes a mutable variable marked `mut` at the call (`restock(mut line, 5)`), which is updated when the call returns. The marker is required there, and an error anywhere else;
- a method that changes its receiver takes `mut self`.

There are no shared references, so nothing changes behind your back. A declared type holds for the variable's whole life: every assignment is checked against it (`line.qty = 120` fails if `qty` is a `Quantity`; `x = -1` fails if `x` was declared `let mut x: Positive`), at compile time for literals and at runtime otherwise, and so is the value a `mut` argument writes back. Passing a `Positive` variable to a `mut k: Int` parameter is a compile error, since the function could store any `Int` in it. Closures cannot change outer variables.

Lambdas are written with an arrow: `x -> x + 1`, `(acc, x) -> acc + x`, `() -> 7`, with optional parameter types (`(s: String) -> len(s)`; a refined one, such as `(n: Positive) -> …`, is checked on every call, like a named function's) and a block for a longer body (`x -> { ... }`). `return e` leaves the enclosing function or lambda early.

## Errors: `Result`, `Option`, `!`, `?` and `except`

A function that can fail returns a `Result` (or an `Option` for "maybe absent"). Three operators keep the happy path straight:

```rust
fn transfer(from: Account, to: Account, amount: Cents) -> Result[(Account, Account), WithdrawError] {
  let from = withdraw(from, amount)!        // an Err is returned from `transfer` as is
  Ok((from, Account { ..to, balance: to.balance + amount }))
}
fn status_of(q: MemQueue, id: TaskId) -> Option[Status] {
  Some(q.entries.find(e -> e.task.id == id)?.status)  // None is returned from `status_of`
}
let port = parse_int(text) except { 8080 }                  // handle it here: a fallback value
let n = parse(s) except e { return describe_error(e) }      // the error, by name; the block may return
```

- `e!` takes the `Ok` value of a `Result`, or returns its `Err` from the function. The error must fit the function's error type: the same type, or a variant of it, or a member of a union (there is no conversion function).
- `e?` does the same for an `Option`: the `Some` value, or return `None`.
- `e except { fallback }` handles a failed `Result` or a missing `Option` value inline. `except err { ... }` names the error (an `Option` has none). The fallback must produce the same type, or leave with `return`.
- They chain: `load(path)!.lines()`, `find(xs, p)?.name`.

## Built-in types

| Type | Notes |
|---|---|
| `Int`, `Float`, `Bool`, `String` | `Int` never overflows: it is a machine integer while it fits and a big integer when it doesn't (like Python), literals included. `Float` is never NaN or infinite. There is no implicit Int/Float mixing (`float(n)`, `round(x)`) |
| `I8` … `I64`, `U8` … `U64` | sized integers: `Int` refinements with a machine integer's range (`U8 = Int where 0 <= it && it <= 255`), checked and verified like any refinement, so `verify` finds where a `U8` would overflow |
| `()` | the unit type: "no meaningful value", Python's `None` as a *type* |
| `Option[T]` | `None` or `Some(x)`: a value that may be absent |
| `Result[T, E]` | `Ok(x)` or `Err(e)`: an operation that may fail |
| `List[T]` | an ordered sequence, `[1, 2, 3]` |
| `(A, B, …)` | tuples, `(word, 3)`, destructured with `let (w, n) = …`, accessed with `t.0` |
| `Dict[K, V]` | `dict([("a", 1)])`, `get`, `insert`, `remove`, `keys`, `values`, `items`; JSON object when keys are strings |
| `Set[T]` | `set([1, 2])`, `insert`, `union`, `intersection`, `difference`, `to_list` |

`+` adds numbers and joins strings and lists (`"a" + "b"`, `[1] + [2]`). **Strings** are UTF-8 and indexed by character: `len`, `chars`, `get(s, i)`, `slice(s, start, end)` (clamped, as in Python; also on lists), `index_of`, `contains`, `starts_with`, `ends_with`, `split`, `join`, `lines`, `trim`, `replace`, `repeat`, `reverse`, `to_upper`, `to_lower`, `is_digit` / `is_alpha` / `is_space`, `ord` / `chr`, `parse_int`, `matches` (regex). **f-strings** interpolate any expression, shown as by `to_string`: `f"{name} has {len(xs)} items"` (`{{` and `}}` are literal braces). A **format spec** follows a `:`, as in Python: `[[fill]align][+][0][width][.precision]`. `{price:.2}` gives two decimal places (or keeps at most 2 characters of a `String`), `{name:<12}` / `{n:>5}` / `{title:*^20}` pad to a width (numbers align right by default, everything else left), `{n:05}` pads with zeros after the sign, `{delta:+}` always shows the sign. A spec that doesn't suit the type is a check error.

All values are immutable (`insert` returns a new dict). Dicts and sets are kept in canonical order, so equal collections are equal values and serialise identically. `len`, `contains`, `get`, `insert`, `remove`, `sum`, `min`, `max` and `abs` work across the types they make sense for. `strata builtins` lists every builtin with its signature. See `examples/builtins.strata`.

**Not yet:** lazy iterators, generators and async, which will be designed as language features of their own.

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

## Verification

`strata verify` turns every contract into proof obligations and gives each function one of these results:

- **proven** (`✓`): the SMT solver proved every check. `--checks` lists them.
- **fuzzed** (`~`): some checks could not be proven, so the function was run on generated inputs and nothing failed. Each unproven check says why, for example *depends on `fold` is not modelled*.
- **failed** (`✗`): a counterexample. When the solver finds one, it is run in the interpreter first, and reported only if it really fails. Otherwise the fuzzer looks for one.
- **incomplete** (`○`): the function still has a hole.

The checks are exactly the interpreter's runtime checks: a callee's parameter refinements and `requires`, the refinements of constructed values and assigned fields, annotated `let`s, `assert`, division by zero, and the function's own result type and `ensures`. Calls are checked against the callee's contract, never its body, which is how modules meet. A function passed by name (`lines.map(line_total)`) is checked against one arbitrary element of the list.

How it works (`src/smt.rs`): the body is run symbolically, with values as SMT terms. Records and sums become structures of terms, lists are tracked by their length, branches are merged with `ite`, and a loop forgets the variables it changes. Each check becomes the question "can the negation be true under what is known here?", which is sent to Z3 as SMT-LIB text (`verify --smt` prints the queries, termination ones included). `unsat` means proven; `sat` comes with a model, which becomes the counterexample. Anything not modelled (floats, dicts, sets, list contents, most higher-order builtins) becomes an unknown value. That can only make a proof fail, never make a false proof succeed, and the reason is kept for the report. `Int` is a mathematical integer, as it is when the code runs. `matches(s, "regex")` becomes a regular-expression constraint when the pattern uses only literals, classes, groups, alternation and quantifiers.

**Helpers in contracts.** A contract can use an ordinary function, such as `type Digit = Int where small(it)` with `fn small(n: Int) -> Bool { 0 <= n && n < 10 }`. Inside a contract, a helper that terminates and cannot fail is *seen through*: its body is used, not just its contract. So `fn zero() -> Digit { 0 }` is proven and `fn ten() -> Digit { 10 }` is refuted. A helper that may not terminate is never seen through. Unfolding one would let the solver prove false things. A helper marked `terminates` is seen through, and the proof says it relied on that.

**Termination.** `verify` also says whether each function always finishes:

- **terminates**, when shown automatically:
  - no recursion, and only `for` loops;
  - a `while a < b` loop (also `<=`, `>`, `>=`, `!=`) whose bound `b - a` the solver proves goes down on every pass, unless the pass ends the loop;
  - recursion where some argument is a smaller piece of a parameter, taken apart by a `match` (`[_, ..rest] => 1 + length(rest)`);
  - recursion with a *measure*: an integer expression of the parameters that the solver proves is never negative at each recursive call, and smaller for the call's arguments. The candidates are tried automatically:
    - each integer parameter (`fact(n - 1)`, `gcd(b, a % b)`);
    - the length of each list or string parameter (`total(drop(xs, 1))`);
    - for each comparison in the body, the distance between its sides (`100 - n` in `up(n + 1)` under `n <= 100`, `hi - lo` in a binary search);
    - pairs of these compared in order, like words in a dictionary (`(m, n)` for Ackermann's function).

  It also needs everything the function calls to terminate.
- **may not terminate**, with the reason: a `loop`, other recursion, mutual recursion, or a call to such a function. The viewer marks these.
- **assumed**: the function is marked `terminates` (after its contract, `fn f(x: Int) -> Int terminates { … }`). Its callers say they terminate *if* it does.

**Policies.** A package's `strata.toml` says what `verify` requires, each rule as a list of modules (`"*"` for all of them, `"main"` for the root module; a module covers the modules below it):

```toml
[verify]
proven = ["core"]        # every check proven, not just fuzzed
terminates = ["*"]       # every function shown (or marked) to terminate
no_recursion = ["core"]  # no recursion at all, even recursion that terminates
```

Recursion is allowed unless `no_recursion` says otherwise. A run that breaks a rule fails, listing each violation. `verify --require proven,terminates,no-recursion` applies the same rules to everything checked, package or not. `strata ci` rejects a rule that names a module the package doesn't have.

**Cross-check.** A proven function that terminates is still run on a few generated inputs (`--cross-check N`, 20 by default, 0 turns it off). If one of them fails, the prover was wrong, and `verify` says so: the result is a failure *found by the cross-check*, flagged as a bug in the prover. Each run has a step budget. A run that goes past it, or past the recursion limit, proves nothing either way and is set aside: proven code can still be far too costly to run, like Ackermann's function.

Z3 is found on the `PATH` (or `STRATA_SOLVER`). Without it, everything is fuzzed. Each query has a resource limit rather than a timeout, so results are deterministic and can be cached by content hash (with the solver version in the key).

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
- **`strata sync [main]`** merges `origin/main` (or the local branch, without a remote) into the current branch with the driver, whether or not `setup-git` was run. Renames that either side made with `strata rename` (read from both sides' `strata.log`) are then applied to the other side's code, in every file: a call that main added to `area` becomes a call to `rect_area` if your branch renamed it, unless a local named `rect_area` would capture it. Then it runs `strata ci` and commits the merge only if that passes; otherwise the merge is left staged, so you can fix it or `git merge --abort`.
- **`strata ci [--fix]`** is what CI runs: every package and script in the repository checks, every package's exports exist, and every `.strata` file is in canonical format (exactly what `render` prints; `--fix` rewrites it). GitHub doesn't run merge drivers, so `.github/workflows/strata.yml` runs `strata ci` on the pull request's merge ref. Make it a required check, and require branches to be up to date before merging. Then a branch lands only once it was merged with `main` by `strata sync` and the result checks.

## Source layout

| File | What it is |
|---|---|
| `src/ast.rs` | the syntax tree (the canonical representation) |
| `src/lexer.rs`, `src/parser.rs` | text → tree |
| `src/printer.rs` | tree → text (canonical form, used for hashing) |
| `src/types.rs` | type inference, traits, exhaustiveness, holes |
| `src/interp.rs` | interpreter: trait dispatch, mocks, runtime contracts, JSON in/out, traces |
| `src/generate.rs` | value generators derived from types and refinements (used by mocks and fuzzing) |
| `src/smt.rs` | SMT verification: symbolic execution of bodies into proof obligations, Z3 queries, models back to values |
| `src/termination.rs` | termination: call graph, loop bounds, structural and integer recursion; which helpers contracts may see through |
| `src/verify.rs` | compile-time refinement checks on literals; the layered check (prove, confirm counterexamples, fuzz the rest); contract fuzzing |
| `src/store.rs` | content-addressed codebase, dependency graph, patch log |
| `src/merge.rs` | `strata merge-file`: three-way merge per definition, renames |
| `src/package.rs` | packages: `strata.toml`, their files, finding packages and scripts |
| `src/view.rs` | level-of-detail projections and selectors |
| `src/ui.rs`, `src/ui/index.html` | `strata ui`: the live viewer (a std-only HTTP server and one page) |

## Known limitations of this draft

- **Proofs cover integers, strings, records, sums and list lengths.** Loops forget what they change (no invariants yet), list elements are tracked only when the whole list is known to be valid, floats, dicts and sets are not modelled, and trait laws are fuzzed only.
- Refinements on generic type arguments, such as `Option[Quantity]`, are not checked when values are constructed.
- Length conditions on a list are only decided as "empty" or "not empty" (`where len(it) == 1` with `where len(it) >= 2` needs a `[]` or `_` arm).
- **No compilation backend.** It is a tree-walking interpreter.

## Roadmap

1. **SMT verification, continued**: loop invariants (inferred where possible, written where it helps), list contents, trait laws, and an `assumed` result for trusted outside code.
2. **Traits with laws**: e.g. `trait Monoid { … law assoc(a, b, c): … }`; laws are verified or fuzzed for every implementation.
3. **Agent-friendly CLI everywhere**: `--json` on every command, trace queries, and scoped workspaces and leases so several agents can work on different slices at once. The CLI *is* the agent API; no server is needed.
4. **Async and iterators** as language features; `mock record` / `mock replay` for integration tests; deterministic replay debugging.
5. **Viewer**: show which agent is working on which slice, and add a diff view for each patch.
6. **Structural records with row polymorphism**: "anything with `.id: OrderId`".
7. **A compilation backend** (WasmGC or Cranelift).
