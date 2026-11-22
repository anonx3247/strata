# The language

A tour of Strata's features. Strata code is highlighted as Rust, the closest match.

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
  fn save(mut self, o: Order)        // `mut self`: saving changes the store
}

fn ship(mut db: Db, id: OrderId, tracking: NonEmptyString) -> Result[Order, ShipError]
  ensures match it { Ok(o) => o is Shipped, Err(_) => true }
{
  order := db.load(id) or { return Err(NotFound) }
  match order {
    Pending { id, lines } => {
      o := Shipped { id, lines, tracking }
      mut db.save(o)
      Ok(o)
    }
    _ => Err(NotPending),
  }
}

fn refund(o: Order) -> Cents
  ensures it >= 0
  = ?refund_policy          // a typed hole: callers still check

test "a mocked store never breaks ship's contract" {
  mut db := mock Db
  r := ship(mut db, "ord_zzz999", "TRK9")
  assert r is Ok || r is Err
}
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
id := UserId("u_42")      // explicit construction, checked
id.value                     // the underlying String
```

A `distinct` type never mixes with its base or with other distinct types (`OrderId` ≠ `UserId`), it can have trait implementations, and it serialises to JSON as its underlying value.

## Records and defaults

A record's field can have a default, written after its type. Nothing gets a default implicitly.

```rust
type Retry = {
  attempts: Int where 1 <= it && it <= 10 = 3,
  backoff: List[Int] = [100, 200],
  label: String = "retry",
}
r := Retry { attempts: 5 }      // backoff: [100, 200], label: "retry"
d := Retry {}
```

A constructor that leaves out a field with a default gets the default; so does JSON input with the key missing, and a command line without the option. A variant's record fields can have defaults too (`Queued { name: String, priority: Int = 0 }`). A default is checked where the type is defined: it must have the field's type, and a literal must satisfy the field's refinement (`n: Positive = 0` is an error). It sees no variables, not even the other fields, and it is evaluated each time it is used. Generated values (mocks, fuzzing) still generate every field.

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

**A union declares its members or names them.** `type Shape = Circle | Square { side: Float } | Dot` uses the record `Circle` and declares `Square` (a record) and `Dot` (an atom: a type with one value). A bare name is the type of that name when one is in scope, and is declared by the union only when nothing has that name; a member written with fields (`Moved(Int)`, `Square { .. }`) is declared. The same union can be written with a bar before each member, one per line, and `type ShapeError = | Empty | TooBig { max: Int }` declares both its members. A member declared in a union is a type like any other: a function can take `e: TooBig` (and read `e.max`), or return `Result[Shape, TooBig]`, and an atom or tuple-shaped type can be declared on its own too (`type Dot`, `type Moved(Int)`), to be used in several unions. Constructors still need wrapping (`Err(TooBig { max: 9 })`), so `Result[Int, Int]` stays unambiguous. A member declared in a union has its name unique in its module (another module's type of the same name is told apart by its module, as `geo.Circle`). `edit` warns when a new member is named like an existing type (`Circel` next to `Circle`).

**Only bare members flatten.** `type S2 = S | C` with `type S = A | B` is `A | B | C`; `type S3 = D(S) | E` is `D | E`: `D` is a new type holding an `S`.

**A union's type parameters are wrapped**, in a member it declares (`type Option[T] = Some(T) | None`, `type OrError[T] = Value(T) | Problem`) or in a type (`type Many[T] = List[T] | Problem`): never a member on its own (`T | Problem`), nor two members alike (`List[A] | List[B]`), which couldn't be told apart. A union of two types is written `A | B` where it is used; there is no `Either[A, B]`. So a generic union is named by its members too: `List[Int] | Problem` is `Many[Int]` (and renders so), and `impl[T] Show for Many[T]` applies to it.

**A named union names a set of values.** A member fits it without wrapping, and the same set written out (`x: Circle | Square | Dot`) is that type: `strata render` writes it as `x: Shape` (named subsets too: `B | C | D` is `A | D` when `type A = B | C`). A named union can have `impl`s, methods and `derive`s; for a value typed by it (or by its set), its own apply, and for a value of a member, the member's own. Two modules may name the same set (`geo.Shape`, `ui.Figure`), but not both implement a trait (or have a method of the same name) for it: a value of the set wouldn't know which applies. A union that adds nothing is refused as a bare alias is: one that is a single type once normalized (`type U = Int | Positive`), or a second name in its module for a set already named there, in the root module or in the prelude (`type Answer = Some(Int) | None` is `Option[Int]`). `distinct type Answer = Some(Int) | None` is a separate type, built as `Answer(Some(3))` and read with `.value`.

`strata render` shows a type that one union lists inside it, unless it has a doc, an `impl` or a `derive` of its own (then on its own); `--members separate` shows each member on its own instead (in `strata ui`: *union members: inline (switch)*). Both read back as the same program.

**Patterns are sets of values**, and an arm takes what the arms above it left:

- a type's name: `Positive =>`, `String =>`, `List[Int] =>`, `Circle =>` (a variant, whatever its fields), or a trait (`Named =>`, values that implement it);
- a variant or record with fields, as before: `Some(x)`, `Pending { id, .. }`;
- a literal: `0`, `"x"`, `true`;
- a condition on the value `it`: `where 3 <= it && it <= 5 =>`, `where len(it) > 0 =>`, `where it < 0.5 =>`;
- alternatives: `Square | Triangle =>`, `0 | 1 | 2 =>`, `(0, x) | (x, 0) =>` (they bind the same names);
- `_`.

**Narrowing.** Inside an arm, the matched variable has the arm's type (`s` is a `Square` above), and no new name is needed. To match the result of an expression, name it: `match lines(text) as ls { … }`. A matched field (`match t.status { Closed => t.status.why, … }`) narrows too. Assigning to a narrowed `mut` variable ends its narrowing.

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

**Capabilities are traits too.** There are no separate effects: a backend (database, queue, console, clock) is a trait, and code that uses one takes an implementation as a parameter or holds it in a field: `fn ship(mut db: Db, ...)`, `type PrintWorker = { console: Console }`. A trait used as a type means "any implementation": `db: Db` is shorthand for `fn ship[D: Db](db: D, ...)`. Name the type parameter only when it must tie things together, as in `combine_all[T: Combine](first: T, rest: List[T]) -> T`. A method that changes its value says `mut self` (`fn save(mut self, o: Order)`). Tests pass a fake implementation or `mock Db`. Iterators and tasks are language features of their own, not capabilities (see Iterators, and Tasks).

**Mocks: fakes for tests.** A fake is declared like a type, with `mock` in place of `type`, and implemented like any type; its implementations may leave methods out:

```rust
/// An in-memory disk, for tests.
mock MemDisk = { files: Dict[String, String] = Dict[String, String] {} }

impl Files for MemDisk {
  fn read(self, path: String) -> Result[String, FsError] { Ok(get(self.files, path) ?? "") }
  fn write(mut self, path: String, text: String) -> Result[(), FsError] {
    self.files = insert(self.files, path, text)
    Ok(())
  }
  // exists, list, ...: left out, generated
}

test "saving then loading" {
  mut fs := MemDisk {}
  assert save_then_load(mut fs, "hi") == Ok("hi")
}
```

- A method left out answers as `mock Files` does: it checks the method's `requires` and gives a result within its type and `ensures`.
- A mock is for tests only: a `test`, another mock, or an `impl` for one may name it; anything else is an error, so a fake never reaches a real run.
- `verify` reports a mock's methods as assumed (the tests that use it check them). `strata view` and the viewer hide mocks and their implementations unless asked (`view --mocks`, "show mocks").
- A mock holds data: a record (`mock FixedClock = {}` for none).

**Capabilities given by the runtime.** A trait that reaches the outside world is marked `capability trait`; anything may implement it, fakes for tests included. The real thing is a `capability`: functions that reach the outside world, holding no data. Only the runtime makes one, for `main` or a test that asks for it by name, and its functions are called on that value, so holding it is the permission:

```rust
/// Reading the time.
pub capability trait Clock {
  fn now(self) -> Int
}

/// The machine's clock.
pub capability SystemClock {
  fn seconds() -> Int { ... }
}

impl Clock for SystemClock {
  fn now(self) -> Int { self.seconds() }
}

/// Any clock: the system's, or a fake in a test.
fn late(c: Clock) -> Bool { c.now() > 40 }

fn main(sys: SystemClock) {
  print(f"{sys.seconds()} {late(sys)}")
}

test "the real clock" (sys: SystemClock) {
  assert late(sys)
}
```

- A capability has no constructor: `SystemClock {}`, `mock SystemClock`, JSON input and `SystemClock.seconds()` are all errors. A program's signature (`main`'s parameters) lists every capability it can use.
- `main` names the capabilities it may use and picks among them in ordinary code (an option choosing SQLite or Postgres), handing the rest of the program a capability trait value (`late(sys)`).
- A test may ask for capabilities the same way (`test "..." (sys: SystemClock) { ... }`); one that doesn't uses fakes.
- `verify` never runs a capability's code (it reports it as assumed), nor code that takes one; that code is proven from the capability's contracts like any other.
- The viewer colours capabilities and capability traits apart from other types.

std's are capabilities too: `Disk` (`std.fs`, for `Files`), `Process` (`std.process`, for `Environment`) and `Launcher` (`std.process`, for `Commands`: running other programs), which `main` names. `Commands` has one method, `run(mut self, program: String, args: List[String], dir: String) -> Result[Finished, String]` (`Finished = { code: Int, out: String, err: String }`): it starts `program` (looked up in `PATH`) with `args`, never through a shell, in the folder `dir` (`""`: the current one), with nothing on its standard input, waits, and gives its exit code (128 + the signal for one killed by a signal) and what it wrote; `Err` only when it can't be started (no such program or folder). Its `ensures` says the code is never negative, which is all `verify` assumes of a call. Code that takes `Commands` is tested with `mock Commands` or a mock type that answers the calls the test expects. `Console` is the one capability the environment chooses, so `main` asks for the trait: it gets the terminal under `strata run`. A test may ask for it too (`test "..." (mut console: Console) { ... }`): what it writes is the test's own output, shown only if the test fails. Under `strata run --trace`, the console's calls are in the trace with the rest, what was written and what was read.

**How the program is run** is hidden by default, so a program behaves the same everywhere. A package that needs to know says so in `strata.toml`:

```toml
[run]
mode = true
```

Then `main` and tests may ask for a `RunMode` (`std.process`): `Running` under `strata run` or in a built executable, `Testing` in a test, `Tracing` under `--trace`. Without it, asking is an error that says how to allow it.

**A type's own methods.** `impl Item { ... }` gives a type methods of its own, with no trait. A method with `self` is called on a value (`item.priced(42)`). One without `self` is called on the type, which makes it a constructor: `Item.new("tea")`. Only a record, sum or `distinct` type defined in the program can have them, in the module that defines it, and in a single `impl Item` block.

```rust
impl Item {
  fn new(name: NonEmptyString) -> Item { Item { name, cents: 0 } }
  fn priced(self, cents: Natural) -> Item { Item { ..self, cents } }
}
tea := Item.new("tea").priced(42)
```

**Shared method names.** Two traits may declare methods with the same name, and each implementation keeps its own body. A call picks by what is known about the value it is called on:

- known only through a trait (`d: Describe`, `T: Describe`): that trait's implementation;
- a known type: its own method if it has one (`impl Item { fn describe(self) … }`), else the one trait of the type that declares the name. This holds even when a function of another module has the same name: inside your own `fn handle`, `app.handle(req)` is `Router`'s method (`handle(app, req)` would be the function);
- otherwise it is a compile error: a type implementing two traits that both have `describe`, with no `describe` of its own; a value known only as `Describe | Log` or `T: Describe + Log`. In the last two cases a type's own `describe` doesn't help: it says what `describe` means on an `Item`, not which trait's meaning the call wants.

This holds whichever modules the traits come from: with `Console` (`std.io`) and `Files` (`std.fs`) both imported, `mut console.write(s)` and `mut files.write(path, s)` each mean their own trait's `write`. A method named like a builtin (`Files.remove`) doesn't hide the builtin: on a value whose type has no trait declaring the name (a dict, a list, a string…), `d.remove(k)` and `remove(d, k)` are the builtin's.

`strata` decides this when checking and writes the decision into the program it runs, so the same call always runs the same implementation. A name that depends on the receiver can't be passed as a bare function value (`map(describe)`): pass a lambda (`x -> x.describe()`).

**`Self` or the trait as a parameter**: `fn bigger(self, other: Self)` takes a value of the receiver's own type, `fn bigger(self, other: Shape)` any implementation. On a trait value (a `Shape`, of any implementation) a method taking `Self` can only be given the value itself or what its `Self`-returning methods make (`s.bigger(s.scaled(2))`); `xs[0].bigger(xs[1])` is an error, since `xs[1]` may be another implementation.

**Trait methods without `self`** are called on a type, never on a value: `Money.empty()`, `T.empty()` in a generic function, `Self.empty()` in a trait's default methods and laws. They are constructors and the like (`fn empty() -> Self`, `fn parse(s: String) -> Result[Self, ParseError]`), so the signature must mention `Self`: one that doesn't couldn't pick an implementation, and should be a plain function. In a generic impl's methods they can be called on the impl's own type parameters too: `impl[T: Zero] Zero for List[T] { fn zero() -> Self { [T.zero()] } }`, or `fn padded(self, n: Int) -> Self { .. push(out, T.zero()) .. }` in `impl[T: Zero] Pad for List[T]`. Types are passed whole when the code runs: the type a method is called on (`List[Int]`), or the static type of the value it is called on, says what `T` is. (Only a value held as a trait type, `d: Describe`, doesn't carry its whole type.) A refined alias stays itself in a type argument: in `read[List[Port]]`, `T` is `Port`, whose methods are `Int`'s; so does one with type parameters (`type Many[T] = List[T] where len(it) >= 2`; `Many[Int]` has `List`'s methods) and a refinement written as a type argument (`List[Int where it % 2 == 0]`, shown as written in messages). **`T.check(x)`**, on any type (a type parameter: the type it is when the code runs), is `Ok(())` when `x` meets the type's refinements, else `Err` saying what it breaks: `Port.check(0)` is `Err("0 does not satisfy `Port` (where 1 <= it && it <= 65535)")`.

```rust
trait Empty: Combine {
  fn empty() -> Self
  law identity(a: Self): Self.empty().combine(a) == a
}
fn combine_any[T: Empty](xs: List[T]) -> T { fold(xs, T.empty(), (acc, x) -> acc.combine(x)) }
```

The type is always named, so which implementation runs is always clear. A generic function that calls such a method is given its type arguments when it runs: each call passes the concrete type the checker inferred (or forwards the caller's own type parameter). So the type must be known at each call: `combine_any([])` alone is an error (annotate: `let xs: List[Money] = []`), and such a function can't be passed as a value (`map(combine_any)`): use a lambda. A trait-typed parameter (`xs: List[Empty]`) has no name to call `.empty()` on: write the type parameter when you need it.

## Modules: units that meet only through contracts

A **file is a module**, named after its path in the package: `shop/orders.strata` is `shop.orders`, and `main.strata` is the root module. There are no `module { }` blocks.

```rust
// orders.strata
pub type Order = { id: OrderId, lines: NonEmpty[Line] }
pub fn ship(mut db: Db, id: OrderId) -> Result[Order, ShipError]  ensures ...
fn helper() -> Int { ... }            // private: invisible outside `orders`

// billing.strata
from orders use Order, ship           // the names, unqualified
use std.time                          // the module: `time.now()`, `time.Clock`
use std.json as j                     // under another name: `j.parse(s)`
pub fn invoice(o: Order) -> Cents { ... }
```

- **Every other module comes in through a `use`**, std's included: `use m` binds the module to its last name (`use shop.orders` gives `orders.ship(x)`), `use m as a` to `a`, and `from m use x, y as z` brings names in unqualified (a type with its constructors, a trait with its methods). A name written in full (`std.json.parse`) still needs a `use` of its module (`use std` covers `std.json` too); without one it is an error saying which `use` to add. Only the prelude (`Option`, `Result`, `Int`, `print`, …) needs none. A long `from` list is printed filled to the line width.

- A module is used **only through its public contract**: `pub` types, function signatures, `requires`/`ensures`, trait laws. Anything else is private, and using it from another module is an error ("`secret` is private to module `a`").
- `strata interface orders` prints that contract, with a hash. Editing a private definition, or a function body without touching its signature, is reported as "implementation only": nothing outside the module can be affected. Changing a `pub` signature or type says so ("module core: its public contract changed").
- **A file is a module.** In a folder, each `.strata` file (a binary module file) is a module named after its path (`shop/orders.strata` → `shop.orders`, `main.strata` → the root module). `render MODULE` prints one module as text, and `render --split DIR` writes the codebase's text out one `.st` file per module. A view that spans several modules (`view a,b`) labels each group with a `// ── module m` comment. In the database, definitions are stored under qualified names, so `orders.Line` and `billing.Line` can coexist. `edit m` targets a module, and `view module:m` shows one.
- The viewer groups the sidebar by module (private definitions in italics) and lays the type map out as one region per module, with the modules others depend on placed first.
- `examples/tasks/` is now four modules (`core`, `memory`, `print`, `sim`) that use each other only through their contracts.

## Mutation: immutable by default, mutable when you say so

```rust
fn total(lines: List[Line]) -> Natural {
  mut sum := 0
  for line in lines { sum += line.qty * line.price }
  sum
}
fn restock(mut line: Line, extra: Int) { line.qty += extra }   // changes the caller's variable
restock(mut line, 5)                                           // and the call says so
```

Procedural code is normal Strata: `loop { … }` repeats until `break`, and `continue` skips to the next iteration. A loop can state what stays true from pass to pass, between its header and its body: `while i < n` then `invariant 2 * total == i * (i + 1)` on the next line. It is checked at the loop's head when the program runs, and `strata verify` proves it (see [verification](verification.md#loops)); most loops need none, as `verify` infers simple ones. Method chains can be split across lines:

```rust
best := scores
  .filter(s -> s > 0)
  .sort()
  .reverse()
  .take(3)
```

**Bindings.** `:=` names a new value and infers its type: `n := 0`, `(w, k) := pair()`. With a type, the colon is already there, so the sign is `=`: `let x: Int = 5`. A mutable variable is `mut x := 0` or `mut x: Int = 0`. A plain `=` changes an existing variable or field (`x = e`, `line.qty = …`), and `+=`, `-=`, `*=`, `/=`, `%=` are short for `x = x + e` and so on (and are how such an assignment is printed). A function that returns `()` leaves out `-> ()`: `fn log(s: String) { … }`. A call's parentheses are written against the name: `f(x)` is a call, while `f (x)` is `f` followed by `(x)`.

Variables are immutable unless declared `mut`. `while` and `for` loops are available. Mutation is always *visible*:
- a function that changes an argument declares it `mut`, and the caller passes a mutable variable marked `mut` at the call (`restock(mut line, 5)`), which is updated when the call returns. The marker is required there, and an error anywhere else;
- a method that changes its receiver takes `mut self`, and the call is marked the same way: `mut queue.claim(2)` (as Hylo writes `&x.append(1)`). The mark is required for such a method, and an error on any other. It marks the first call on the place after it: `mut queue.claim(2).len()` changes `queue` through `claim`, and is written `(mut queue.claim(2)).len()`. As an argument, the marked call is put in parentheses, `f((mut queue.pop()), x)`, to tell it from `f(mut queue)`, which passes `queue` to a `mut` parameter.

Only a variable declared `mut`, a `mut` parameter, or a field of one can be changed: passed `mut`, or marked as a receiver. A call's result is a value, not a place, and the changes would have nowhere to go: `mut first(xs).tick()` is an error. Name it first (`mut x := first(xs)`).

There are no shared references, so nothing changes behind your back: `ys := xs` is a copy, and changing `xs` afterwards leaves `ys` as it was (the same holds for passing a value or storing it in a field). There is no `copy` or `clone` to call; the copying is the compiler's business (compiled code shares the value and copies it only when one side changes it while the other still holds it). A declared type holds for the variable's whole life: every assignment is checked against it (`line.qty = 120` fails if `qty` is a `Quantity`; `x = -1` fails if `x` was declared `mut x: Positive`), at compile time for literals and at runtime otherwise, and so is the value a `mut` argument writes back. Passing a `Positive` variable to a `mut k: Int` parameter is a compile error, since the function could store any `Int` in it. Closures cannot change outer variables.

**Ranges, slices and comprehensions.** `0..n` is the integers from `0` to `n - 1`, and `1..=n` includes `n`; a range is a `List[Int]` (`for i in 0..n { … }`, `(0..n).map(…)`). A list is sliced with a range: `xs[1..3]`, `xs[..3]`, `xs[2..]`, `xs[..=i]`, clamped to its length as in Python (so is a finite iterator, such as a string's units: `s.graphemes()[..3]`). There are no negative indices: the end of `xs` is `len(xs)`, as in `xs[..len(xs) - 1]`. One element is `xs[i]`: `i` must be in range, which is checked when the code runs and proven by `verify` (`xs[0]` on a `NonEmpty[Int]` is proven; `xs[len(xs) - 1]` on any list is not). `get(xs, i)` gives an `Option` instead. A comprehension builds a list from one or more others, with a `where` to keep only some elements, as in a refinement:

```rust
[x * x for x in xs where x > 0]
[(a, b) for a in 1..=3 for b in ["x", "y"] where a != 2]
[(name, n) for (n, name) in pairs]
Dict { w: len(w) for w in words }
Set { x % 3 for x in xs }
```

They are written in the tree as such, so they render back as written. When the program is resolved they become the builtins (`range`, `slice`, `map`, `filter`, `fold`), so the checker, the compiler and the prover treat them exactly as those calls.

Lambdas are written with an arrow: `x -> x + 1`, `(acc, x) -> acc + x`, `() -> 7`, with optional parameter types (`(s: String) -> s + "!"`; a refined one, such as `(n: Positive) -> …`, is checked on every call, like a named function's) and a block for a longer body (`x -> { ... }`). `return e` leaves the enclosing function or lambda early.

## Errors: `Result`, `Option`, `!`, `?`, `except` and `or`

A function that can fail returns a `Result` (or an `Option` for "maybe absent"). Each has its own pair of operators: `!` and `except` for a `Result`, `?` and `or` for an `Option`.

```rust
fn transfer(from: Account, to: Account, amount: Cents) -> Result[(Account, Account), WithdrawError] {
  from := withdraw(from, amount)!        // an Err is returned from `transfer` as is
  Ok((from, Account { ..to, balance: to.balance + amount }))
}
fn status_of(q: MemQueue, id: TaskId) -> Option[Status] {
  Some(q.entries.find(e -> e.task.id == id)?.status)  // None is returned from `status_of`
}
port := parse_int(text) ?? 8080                          // an Option: its value, or a fallback
user := find_user(id) or { return not_found() }          // ... or a block, which may return
n := parse(s) !! 0                                       // a Result: its value, or a fallback
n := parse(s) except e { return describe_error(e) }      // ... or a block, with the error by name
```

- `e!` takes the `Ok` value of a `Result`, or returns its `Err` from the function. The error must fit the function's error type: the same type, or a variant of it, or a member of a union (there is no conversion function).
- `e?` does the same for an `Option`: the `Some` value, or return `None`.
- `e except { fallback }` handles a failed `Result` here: the `Ok` value, or else the block. `except err { ... }` names the error. The fallback must produce the same type, or leave with `return`.
- `e or { fallback }` does the same for an `Option`: the `Some` value, or else the block (`None` has nothing to name).
- `e !! value` and `e ?? value` are the short forms, for a fallback that is a single value on one line; such a fallback is always written that way. They bind tighter than comparisons and looser than arithmetic (`n ?? 0 + 1` is `n ?? (0 + 1)`, `n ?? 0 == 1` is `(n ?? 0) == 1`), and chain: `a ?? b ?? 0`.
- Each works on its own type only: `except` / `!!` on an `Option`, or `or` / `??` on a `Result`, is an error. So a `Result[Option[T], E]` reads one way: `(load(k) !! None) ?? default` handles the error, then the missing value.
- They chain: `load(path)!.lines()`, `find(xs, p)?.name`.

## Tasks: `scope`, `spawn` and `await`

Work runs in parallel in tasks. A `scope` starts them and ends only once every task in it has finished:

```rust
fn fetch_both(net: Net) -> (Page, Page) {
  scope {
    a := spawn fetch(net, url1)
    b := spawn fetch(net, url2)
    await (a, b)
  }
}

squares := scope { await [spawn x * x for x in xs] }
```

- `spawn e` runs the expression `e` (all of it, as `return e` takes all of it) in a new task, and gives a handle, `Task[T]`. It must be inside a `scope`.
- `await h` waits for the task and gives its result; `await (a, b)` gives a tuple of results, `await hs` a list. It takes what follows it like `-` does, so `(await hs).len()` needs the parentheses.
- There is no `async` function: any function can be called in a task, and a call that waits (a read, a long computation) is a plain call. Nothing about a function says whether tasks run it.

Tasks share nothing that can change, so the result doesn't depend on timing: `scope { a := spawn f(); b := spawn g(); await (a, b) }` is `(f(), g())`, and `verify` reasons about it as such. What a task uses from around it is a copy at `spawn` time, as in a lambda: it can't change outer variables (`spawn { n += 1 }` is an error), and `return`, `!` and `?` can't leave it (handle the error in the task, or make it the task's result).

A handle stays in its scope: a scope can't give one back, a task can't hold another's, and one can't be stored in a variable declared outside. A fault in a task (an index out of range, a broken contract) ends the program at once, as anywhere else: nothing catches it, and the other tasks don't run on (a thread waiting for what the failed task would have sent doesn't hang). Its call stack shows where the task was started and where it failed: `main -> spawn -> pick`. Under `strata test` it fails that test only: the test's other tasks stop at their next wait, and the next test runs.

**A deadlock is a panic, not a hang.** When every task of the program is waiting (in a `recv`, a `send`, a `select`, an `await`, the end of a `scope`, a fake clock's sleep) and none of the waits can end by itself (a real clock's sleep or `after` timer can: it isn't a deadlock), nothing can ever happen again. The program stops with what each task waits on, and where: `deadlock: every task is waiting, so none can go on: main waits in the end of a `scope`, at main; a task waits in `recv`, at main -> spawn -> Receiver.recv; …`. Under `strata test` the test fails with it. A task reading the console or a file isn't waiting in this sense: it can go on by itself.

A `scope` whose last line is a `spawn` (`scope { spawn work() }`) gives `()`: the handle isn't its value. Tasks run on threads, one per task for now.

### Channels and `select`

Tasks pass values to each other through channels. `channel(n)` gives a channel's two ends, with room for `n` values waiting; `channel()` has none, so a `send` waits until a receiver takes the value. The type of what goes through is inferred from its use.

```rust
fn squares(jobs: List[Int]) -> Int {
  (tx, rx) := channel(16)
  (out, results) := channel()
  scope {
    spawn {
      for j in jobs { tx.send(j) }
      tx.close()
    }
    for _ in 0..4 {
      spawn {
        for j in rx { out.send(j * j) }
        out.close()
      }
    }
    tx.close()
    out.close()
    mut total := 0
    for r in results { total += r }
    total
  }
}
```

- `tx.send(v)` moves `v` into the channel, waiting while it is full. `rx.recv()` gives `Some(v)`, or `None` once the channel has ended and is empty; `for v in rx` takes every value, then stops.
- **Closing is explicit, once per task.** Each task holding the sender (the one that made the channel, and every `spawn` that uses it) calls `tx.close()` when it is done sending; the channel ends when all of them have. The checker requires it where the channel is made and in each task that uses the sender; a task that still ends without closing (the call was in a branch not taken) has its copy closed for it, and that is a fault. A `spawn` takes a sender only as a variable of its own, not inside a tuple or record.
- Any number of tasks can send to one channel and receive from it. `Sender[T]` and `Receiver[T]` are the types of the two ends.

`select` waits on several operations and does the first one that can happen, and only that one:

```rust
loop {
  select {
    jobs.recv() as j => match j {
      Some(x) => run(x),
      None => break,
    },
    results.send(last) => {
      sent += 1
    },
    await download as d => show(d),
  }
}
```

An arm is `rx.recv()` (its `as` name is an `Option`), `tx.send(v)` (nothing to name) or `await h` (the task's result). An arm that doesn't happen has done nothing: a value it would have sent isn't sent, and a task it awaits keeps running. A `recv` on a channel that has ended is always ready (with `None`), so a loop over several channels stops looking at one once it ends. When several arms can happen, which one does is not fixed.

### `race`

`race` has the same arms as `select`, but each arm's left side is any expression, run as a task of its own. The first to finish runs its arm, with its result as the `as` name; the others are cancelled:

```rust
fn first_answer(mirrors: Receiver[Page], local: Cache, key: String) -> Page {
  race {
    mirrors.recv() as p => p ?? not_found(key),
    lookup(local, key) as hit => hit,
  }
}
```

A cancelled task stops at its next wait (a `recv`, a `send`, an `await`, a `select`, a `sleep`, a console's `read_line`), however deep in calls it is, and leaves as `return` would: what it holds is dropped, nothing after the wait runs, and the tasks it started stop too. Code that doesn't wait runs to its end (a long computation isn't interrupted halfway). The `race` ends once every loser has stopped, then runs the winner's arm. A left side follows a `spawn`'s rules (no `return`, `!` or `?` out of it; a sender it uses is its own copy, to close).

### Supervised tasks

A panic ends the program, wherever it happens. A task started with `supervised spawn` is the exception: its panic ends only that task (and the tasks it started), and awaiting it gives a `Result[T, Panic]` instead of a `T`. The failure is handled where the task is awaited, with what handles any `Result`:

```rust
fn page(net: Net, url: String) -> Page {
  scope {
    h := supervised spawn fetch(net, url)
    await h except p {
      log(f"{url}: {p.message}")
      cached(url)
    }
  }
}
```

- `Panic` is `{ kind: String, message: String, stack: String }`: the kind (`Runtime`, `Requires`, `Refinement`, …), the message, and the call stack from `main` through each `spawn`.
- A handler after `await h` is the result's: `await h except p { … }` is `(await h) except p { … }`, and `await h!` is `(await h)!`.
- When a supervised task panics, every task below it stops at its next wait; its copies of senders are closed (its receivers see the channel end); what it held is freed (a division by zero or a bad builtin argument included; only a stack overflow or running out of memory leaves it allocated); its siblings and the task that started it go on. A panic in an ordinary task below it is the supervised task's.
- In a `race`, an arm can be supervised the same way: `race { supervised fetch(a) as r => …, … }` (`r` is then a `Result[T, Panic]`, and a panic is that arm's result).
- `supervised` is a keyword only right before `spawn` (and at the start of a `race` arm): it is a name anywhere else.

### Time: `Clock`

Time comes from outside the program, so it goes through a capability (`std.time`): `main` or a test asks for the `SystemClock` and passes it on (as a `Clock`). `clock.now()` is an `Instant` (monotonic: only the time between two means something, `elapsed(a, b)`), and `clock.sleep(d)` waits for a `Duration` (`nanos`, `micros`, `ms`, `seconds`; `.value` is nanoseconds). Sleeping is a wait like a `recv`: a cancelled task stops sleeping at once. So a `race` makes a timeout:

```rust
fn fetch_or_give_up(clock: SystemClock, net: Net, url: String) -> Option[Page] {
  race {
    fetch(net, url) as p => Some(p),
    clock.sleep(seconds(2)) => None,
  }
}
```

In a `select`, `clock.after(d) => …` is an arm that happens once `d` has passed, in that clock's time, since the `select` began (it names nothing): `select { jobs.recv() as j => run(j), clock.after(ms(100)) => flush() }`. Any `Clock` works, so a test can pass a stand-in whose time it controls (one that stands still, or runs faster): its `after` arms and its `sleep` follow its time, not the system's.

A test of timeouts asks for a `VirtualClock`, a capability like `SystemClock`: each test (and the program, for `main`) has its own virtual time, at 0 when it starts and shared by its tasks, and it moves by itself: once every task is waiting and one of them waits for this clock (a `sleep`, an `after` timer), its time jumps to the nearest such deadline. A timeout then happens at once, and exactly:

```rust
/// Waits for a question; none ever comes.
fn answer(questions: Receiver[Int]) -> Int {
  questions.recv() ?? 0
}

test "gives up after 30 seconds" (clock: VirtualClock) {
  (ask, questions) := channel()
  r := race {
    answer(questions) as v => Some(v),
    clock.sleep(seconds(30)) => None,
  }
  ask.close()
  assert r == None
  assert clock.now().value == seconds(30).value
}
```

Sleeps of a virtual clock end in the order of their deadlines, each once everything else is waiting, so a test of timers is exact and takes no real time.

A test that needs a time that stands still writes its own mock of `Clock` (std has none: it is a few lines, and each test knows the time it wants; a mock is for tests only):

```rust
/// The time on a receipt, in seconds.
fn stamp(clock: Clock) -> String {
  f"at {clock.now().value / 1000000000} s"
}

/// A clock stopped at one time.
mock StoppedClock = { at: Instant }

// (`sleep` is left out: a mock's missing methods are generated)
impl Clock for StoppedClock {
  fn now(self) -> Instant {
    self.at
  }
}

test "a receipt shows the time" {
  clock := StoppedClock { at: Instant(seconds(5).value) }
  assert stamp(clock) == "at 5 s"
}
```

## Built-in types

**`core`.** What every program has without a `use` is declared in `core/`, a package below the standard library that depends on nothing (std depends on it): `core.types` (the types below, `Debug`, `Display`, `Option`, `Result`, the refinements), `core.builtins` (the builtin functions), and code modules a program gets only when it uses them: `core.iter` (iterators, when it names one of theirs: `xs.iter()`, `impl Iterator`, ...; a list's `map` is a builtin and needs none), `core.channel` (`channel()`, `Sender`, `Receiver` and their methods), `core.text` (`s.bytes()`, `s.code_points()`) and `core.task` (`Panic`, for `supervised spawn`). `core.mem` is raw memory (`Ptr`, `raw_alloc`, `load_i64`, `store_u8`, `atomic_add_i64`, ...), what the runtime is to be written in: only core's own modules can use it (anywhere else, `Ptr` and its functions are errors), and `strata --file core test` runs core's own tests. What the compiler itself provides is declared with `builtin`, which only `core` may use: a type with its parameters (`builtin type Dict[K, V]`) and a function's signature, with `requires` where it fails on some arguments, so a builtin's precondition is a contract like any other, and one without `requires` always returns:

```rust
/// `xs[i]`: element `i`, from 0.
builtin fn at[A](xs: List[A], i: Int) -> A
  requires 0 <= i && i < len(xs)

/// The string of one character, code point `c` (not a surrogate).
builtin fn chr(c: Int) -> String
  requires 0 <= c && c <= 1114111 && !(55296 <= c && c <= 57343)
```

A name may have several signatures (`len` of a list, a dict or a set): the first whose parameters fit the arguments is called. `strata --file core render builtins` shows them all (`strata builtins` lists them in short); `strata --file core/ edit MODULE < text` stores a module of core as written (its names are global, nobody's module's), and `strata --file core check` checks the core strata is built with. Not yet contracts: `matches` with a pattern that isn't a regular expression, and a channel used after it ended.

| Type | Notes |
|---|---|
| `Int`, `Float`, `Bool`, `String` | `Int` never overflows: it is a machine integer while it fits and a big integer when it doesn't (like Python), literals included. `Float` is never NaN or infinite: an operation whose result would be is a fault (`1e308 * 10.0`). Where that can't be ruled out (code that runs other programs' arithmetic), `checked_add`, `checked_sub`, `checked_mul`, `checked_div`, `checked_rem`, `checked_sqrt` give an `Option[Float]` and `checked_floor`, `checked_ceil`, `checked_round` an `Option[Int]`, `None` instead of the fault. There is no implicit Int/Float mixing (`float(n)`, `round(x)`) |
| `I8` … `I64`, `U8` … `U64` | sized integers: `Int` refinements with a machine integer's range (`U8 = Int where 0 <= it && it <= 255`), checked and verified like any refinement, so `verify` finds where a `U8` would overflow |
| `()` | the unit type: "no meaningful value", Python's `None` as a *type* |
| `Option[T]` | `None` or `Some(x)`: a value that may be absent |
| `Result[T, E]` | `Ok(x)` or `Err(e)`: an operation that may fail |
| `List[T]` | an ordered sequence, `[1, 2, 3]` |
| `(A, B, …)` | tuples, `(word, 3)`, destructured with `(w, n) := …`, accessed with `t.0` |
| `Dict[K, V]` | `Dict { "a": 1, "b": 2 }`, `Dict[String, Int] {}`, `dict(pairs)` from a list of pairs, `get`, `insert`, `remove`, `keys`, `values`, `items`; a JSON object when keys are strings |
| `Set[T]` | `Set { 1, 2 }`, `Set[Int] {}`, `set(xs)` from a list, `insert`, `union`, `intersection`, `difference`, `to_list` |

`+` adds numbers and joins strings and lists (`"a" + "b"`, `[1] + [2]`). **Bits**: `&`, `|`, `^` work on `Int`s bit by bit, as two's complement infinitely sign-extended (as in Python: `-1 & x` is `x`, big integers included), `a << n` is `a` times 2^n and `a >> n` is `a` divided by 2^n rounded down (`-5 >> 1` is `-3`); a negative `n` is a fault, which `verify` looks for. They bind as in Rust and Python, tighter than comparisons (`x & 255 == x` is `(x & 255) == x`): `|`, then `^`, then `&`, then the shifts, then `+` / `-`. A bit operation compared is shown in parentheses, `(x & 255) == x`, since C (and so most readers) puts `==` first. In a refinement or a `where` pattern, where `|` separates a union's members or a pattern's alternatives, a bit-or must be in parentheses (`where (it | 1) == 3 | 0`). **Strings** are UTF-8, and a string isn't counted or indexed itself: the unit is named, `s.bytes()`, `s.code_points()` or `s.graphemes()` (iterators: `len(s.graphemes())`, `s.graphemes()[i]`; see Iterators), and `len(s)`, `s[i]`, `s[a..b]` say so. Positions in a string are byte offsets: `index_of(s, sub)` gives one, and `byte_slice(s, a, b)` cuts at them in constant time (clamped to the string; an offset inside a character moves back to its start), with `byte_len(s)`. String functions don't fail: `split(s, "")` splits at every character (`["", "a", "b", ""]`), `replace(s, "", x)` puts `x` around every character, and `repeat(s, n)` with `n < 1` is `""`. Also `chars`, `contains`, `starts_with`, `ends_with`, `split`, `join`, `lines`, `trim`, `replace`, `repeat`, `reverse`, `to_upper`, `to_lower`, `is_digit` / `is_alpha` / `is_space`, `ord` / `chr`, `parse_int`, `matches` (regex). JSON is `std.json`'s: a type that derives `Json` (or a built-in type) is written with `x.to_json()` and `stringify`. **f-strings** interpolate any expression, shown as by `to_string` (`Display`, below): `f"{name} has {len(xs)} items"` (`{{` and `}}` are literal braces). A **format spec** follows a `:`, as in Python: `[[fill]align][+][0][width][.precision]`. `{price:.2}` gives two decimal places (or keeps at most 2 characters of a `String`), `{name:<12}` / `{n:>5}` / `{title:*^20}` pad to a width (numbers align right by default, everything else left), `{n:05}` pads with zeros after the sign, `{delta:+}` always shows the sign. A spec that doesn't suit the type is a check error. `float_bits` / `float_from_bits` give a float's IEEE 754 bits as an `Int`, and back (`None` for a NaN's).

**Showing values: `Debug` and `Display`.** Every type has two traits without saying so. `Debug` (`debug(x)`, `x.debug()`) writes a value as it is written in code: `Some("hi")`, `Point { x: 1, y: 2 }`, `[1, 2]`, `(1, "a")`, `Dict { "a": 1 }`, `Set { 1, 2 }`, `UserId(7)`, strings quoted. `Display` (`to_string(x)`, `x.to_string()`) is what `print` and f-strings show: a `String` is its text, anything else its `debug`; inside a list, a record or any other value, the parts are shown with `debug` (as Python's `str` of a list uses `repr`), so `["a b"]` and `["a", "b"]` stay apart. A type replaces either with an impl of its own, which is used everywhere it is shown (nested values too):

```rust
impl Display for Money { fn to_string(self) -> String { f"${self.cents / 100}.{self.cents % 100:02}" } }
impl Debug for Secret { fn debug(self) -> String { "Secret { key: *** }" } }
print(Money { cents: 1234 })       // $12.34
print([Money { cents: 5 }])        // [Money { cents: 5 }]: parts by their `debug`
print(Some(Secret { key: "k" }))   // Some(Secret { key: *** })
```

All values are immutable (`insert` returns a new dict). Dicts and sets are kept in canonical order, so equal collections are equal values and serialise identically: numbers and strings in their natural order, a `distinct` value as what it is made of, variants in the order they are declared (then field by field), tuples, records and lists element by element. `len`, `contains`, `get`, `insert`, `remove`, `sum`, `min`, `max` and `abs` work across the types they make sense for. `strata builtins` lists every builtin with its signature. See `examples/builtins.strata`.

**Iterators.** Besides lists, sets and dicts, a `for` loop runs over any value whose type implements the prelude's `Iterator`: `next` gives the elements one at a time, then `None`.

```rust
trait Iterator[T] {
  fn next(mut self) -> Option[T]
}

type Countdown = { n: Int }

impl Iterator[Int] for Countdown {
  fn next(mut self) -> Option[Int] {
    if self.n == 0 { None } else { self.n -= 1; Some(self.n + 1) }
  }
}

c := Countdown { n: 3 }
for i in c { print(i) }            // 3, 2, 1
```

Such a loop works on its own copy of the iterator (`c` is unchanged afterwards), and takes `break`, `continue` and patterns as any `for`. Generic code asks for the trait: `fn collect[T, I: Iterator[T]](it: I) -> List[T]`. `verify` doesn't show that such a loop ends, as it does for a list's: the iterator may never return `None`.

An iterator that ends implements `FiniteIterator` too, a supertrait: its `len` is how many elements `next` still gives.

```rust
trait FiniteIterator[T]: Iterator[T] {
  fn len(self) -> Natural
  fn fold[B](self, init: B, f: (B, T) -> B) -> B   // given
  fn to_list(self) -> List[T]                      // given
}
```

`verify` shows that a `for` loop over a `FiniteIterator` ends, as one over a list: it relies on `len` (which it doesn't check yet). Over an iterator that may be unbounded, it says the loop may not end. `fold` is only on finite iterators: on an unbounded one it would never return (`fold` needs a `FiniteIterator`: `N` is an `Iterator` but not a `FiniteIterator`).

Every iterator has `map` and `filter`, which are lazy: they give another iterator, which does the work as the loop asks for elements, so they work on unbounded iterators too. Mapped or filtered, a finite iterator is still one (a filtered one's `len` counts the elements kept). A function passed to them must end, as the code it is written in must: `verify` follows a lambda's calls from the function it is written in (`it calls spin, which may not terminate`), which is what shows `fold` over a mapped and filtered finite iterator ends.

```rust
type Naturals = { at: Int }   // impl Iterator[Int]: 0, 1, 2, … without end

squares := Naturals { at: 0 }.filter(n -> n % 2 == 0).map(n -> n * n)
for s in squares {
  if s > 100 { break }
  print(s)                     // 0, 4, 16, 36, 64, 100
}
Count { at: 0, end: 5 }.map(n -> n + 1).fold(0, (a, b) -> a + b)   // 15
```

On a list, `xs.map(f)`, `filter` and `fold` are still the builtins, which give a list.

A comprehension over an iterator is an iterator: lazy, so it can be unbounded, and finite when its source is. Set and dict comprehensions collect a finite one.

```rust
odd_squares := [n * n for n in Naturals { at: 0 } where n % 2 == 1]   // 1, 9, 25, … on demand
[x * 2 for x in Count { at: 0, end: 4 }].to_list()                    // [0, 2, 4, 6]
Set { x % 2 for x in Count { at: 0, end: 4 } }                        // Set { 0, 1 }
```

A list, a set or a dict isn't an iterator itself (its `map` and `filter` are the builtins, which give lists), but `iter()` gives one, finite: the prelude's `ListIter`, over a list's elements, a set's in order, or a dict's entries by key. A range is a list, so `(0..n).iter()` works too.

```rust
fn total[I: FiniteIterator[Int]](it: I) -> Int { it.fold(0, (a, b) -> a + b) }

total([3, 1, 2].iter())                  // 6
total((0..5).iter())                     // 10
for (k, v) in d.iter() { … }             // a dict's entries, by key
```

A string isn't an iterator either: its unit is always named, and each is a finite iterator.

```rust
s := "héllo 👋🏽"
s.bytes()          // its UTF-8 bytes, 0 to 255: 104, 195, 169, …
s.code_points()    // its code points (Unicode scalar values): 104, 233, …, 128075, 127997
s.graphemes()      // its grapheme clusters (UAX #29), as strings: "h", "é", …, "👋🏽"
s.graphemes().len()                                   // 7
[chr(c) for c in s.code_points() where c > 127]       // lazily: "é", "👋", "🏽"
```

Each walks the string once: `bytes()` and `code_points()` are the prelude's, read from the UTF-8 bytes (the only native part, `byte_len` and `byte_at`); `graphemes()` is `std.unicode`'s (loaded for a program that calls it), Unicode 16.0's extended grapheme clusters, combining marks, emoji sequences, flags, Hangul syllables and Indic conjuncts included (it agrees with every case of Unicode's GraphemeBreakTest.txt).

A finite iterator is measured, indexed and sliced as a list is, with the same syntax: `len(it)`, `it[i]` (`at`; its precondition is that `i` is in range), `get(it, i)`, `it[a..b]`, `it[a..]`. Slicing is lazy (`drop` then `take`), and `join` collects a finite iterator of strings. Every iterator has `take(n)` (finite, even on an unbounded one) and `drop(n)`. Each walks the iterator from the start, so indexing is linear in `i`.

```rust
s := "héllo 👋🏽!"
len(s.graphemes())                  // 8   (len(s.bytes()) is 16)
s.graphemes()[6]                    // "👋🏽"
join(s.graphemes()[1..3], "")       // "él"
Naturals { at: 0 }.drop(3).take(4)  // 3, 4, 5, 6
```

**Not yet:** a nested comprehension whose inner source is an iterator, and generators.

## Foreign functions: `extern c`

A function written in C is declared with `extern c fn`, a signature with contracts and no body, and a pointer C gives out with `extern c type`. `examples/fuzzy` uses its own C file and libm this way:

```rust
extern c fn edit_distance(a: String, b: String) -> I32
  ensures it >= 0

/// libm's cube root.
extern c fn cbrt(x: Float) -> Float

/// The fewest single-character edits that turn `a` into `b`.
pub fn distance(a: String, b: String) -> Natural {
  edit_distance(a, b)
}
```

```toml
# strata.toml: the C the declarations are linked to
[native]
sources = ["c/text.c"]   # the package's own C files
libs = ["m"]             # libraries (-lm)
```

- **What crosses.** The sized integers (`I8` … `I64`, `U8` … `U64`, C's `int8_t` … `uint64_t`), `Float` (`double`), `Bool`, `String` (copied each way, as a NUL-terminated string), `()` (`void`, as a result), `extern c type` handles, and `Option` of a handle (a pointer that may be NULL: `None`). `Int` doesn't: its size isn't fixed. A `mut` parameter is an out-pointer (`mut db: Option[Sqlite3]` is a `sqlite3 **`): copied in, and back out after the call, as `mut` always is. A `String` can't be `mut`.
- **A handle** (`extern c type Sqlite3`) is a type with no constructor and nothing inside: only C makes one, Strata code passes it back. It isn't counted or freed by Strata (C frees it, through a function you call); two handles are equal when they are the same pointer. A C function that gives NULL where a handle is declared is a fault: declare that result `Option[Sqlite3]`.
- **Faults at the boundary.** Arguments are checked against their types (`I8` is in range) and `requires` before the call; the result against its type and `ensures` after it. A string holding a NUL character can't be passed (C would read it as its end).
- **The name is the C symbol**, so an extern function can't be renamed (wrap it instead).
- **Only its own module calls it.** Extern definitions are never `pub`, and code in other modules (submodules included) can't call them. The module that declares them gives the rest of the program its own functions, or a capability for what touches the outside world, so a signature still shows what code can touch.
- **Contracts are assumed.** `verify` never proves foreign code: a foreign function's contract (`ensures it >= 0`) is taken as true where it is called, and the code calling it is verified like any other (fuzzing runs the compiled code, C included). A compiled program checks that contract at each call, as nothing proves it.
- **Calling C** works wherever code runs: `strata build`, and `strata run` / `strata test` with the JIT, which compiles the `[native]` sources into a library with clang and loads it. C's own library (`strlen`, …) needs no `[native]`.

Not yet: `main` asking for a native module's capability, structs passed by value, callbacks, buffers, and other languages (`extern python`, `extern ts`, `extern rust`, with values crossing as `Json`; see IDEAS.md).

## Programs and the standard library

A program's entry point is `main`. It asks for what it needs by type, and `strata run` passes it in: its command-line options, and the capabilities it uses.

```rust
from std.cli use CommandLine

/// Greet people by name.
type Options = {
  /// Shout the greeting.
  loud: Bool = false,
  /// Say it this many times.
  times: Int where 1 <= it && it <= 10 = 1,
  /// Who to greet (the world if nobody).
  names: List[String] = [],
}

derive CommandLine for Options

fn main(opts: Options) {
  who := if len(opts.names) == 0 { "world" } else { join(opts.names, " and ") }
  for _ in 0..opts.times {
    print(if opts.loud { to_upper(f"hello, {who}!") } else { f"hello, {who}!" })
  }
}
```

```sh
strata run examples/hello -- Ada --loud      # HELLO, ADA!
strata run examples/hello -- --help          # usage, from the doc comments
strata run examples/wc -- --only words README.md
cat README.md | strata run examples/wc
```

**`print(x)`** is always available, with no `use`. It writes `x` (as `to_string` shows it) and a newline wherever the code runs: to the terminal under `strata run`, into the test's own buffer under `strata test` (shown only if the test fails), and nowhere under `verify`. It is a convenience; code whose output a test should check, or that a caller should control, takes a `Console`.

**Command-line options** are a record that derives `CommandLine` (`std.cli`): `derive CommandLine for Options` writes its `from_args` and `help` for that record, at compile time. Each field is read from the arguments by its type: a `Bool` is a flag (`--loud`), an `Option[T]` an option with a value (`--times 3` or `--times=3`), a `List[T]` the remaining positional arguments, and any other field a required positional argument, in field order, or an option (`--times 3`) when it has a default. A field left out takes its default. `T` is an `Int`, `Float`, `String`, a refinement of one, or a choice (a type whose variants carry nothing: `type Count = | Lines | Words` is `lines` or `words`). Names take dashes (`dry_run` is `--dry-run`), `--` ends the options, every refinement is checked (`--times: 0 does not satisfy …`), and a mistake exits with code 2. `--help` is built from the record's and fields' `///` doc comments. `strata check` rejects a `CommandLine` record a command line can't fill.

`main` can also ask for `Console` (`std.io`; the environment chooses it: the terminal) and capabilities, in any order: std's `Disk` (`std.fs`), `Process` and `Launcher` (`std.process`) and `Codebase` (`std.code`: the codebase itself, as the tools see it, a `Code`), or the program's own. It names them, and passes them on where `Files` or `Environment` is wanted: `fn main(mut console: Console, disk: Disk) { wc(mut console, disk) }`, with `wc` taking any `Files` so that tests can pass `mem_files(...)`. It returns `()` (exit code 0), an `ExitCode` (`Int where 0 <= it && it <= 255`), or a `Result` of either: an `Err` is written to standard error and the exit code is 1. A runtime fault exits with 101. `strata run <fn> [ARGS]` still calls a single function (its arguments written as code).

The **standard library** is the `std/` package, written in Strata and built into `strata`, so any program can `use std.…` without declaring a dependency:

| Module | What it has |
|---|---|
| `std.io` | `Console` (`print`, `write`, `eprint`, `read_line`); `Recorder` / `recorder(input)` / `printed(r)` for tests |
| `std.fs` | `Files` (`read`, `read_bytes`, `write`, `append`, `exists`, `list`, `make_dir`, `remove`), `FsError`; `MemFiles` / `mem_files(files)` for tests |
| `std.time` | `Clock` (`now`, `sleep`), the `SystemClock` capability, and the `VirtualClock` capability (its own time per test, moving by itself) for tests; `Duration` (`nanos`, `micros`, `ms`, `seconds`), `Instant`, `elapsed`; `clock.after(d)` as a `select` arm |
| `std.process` | `Environment` (`args`, `var`), `Commands` (`run`, giving a `Finished`) with the runtime's `Launcher`, `ExitCode`; `FakeEnvironment` / `fake_environment(args, vars)` for tests |
| `std.code` | `Codebase`, a capability, and the capability trait it implements, `Code` (`version`, `snapshot`): the codebase in the current folder as the tools see it: each `Definition` with its interface and source text, what it uses and what uses it, holes, errors and cached verdicts, and the package's log. `strata ui`'s viewer reads it |
| `std.cli` | `CommandLine`: a record of options read from the arguments |
| `std.typing` | types as compile-time values, for derivable traits: `Type` (`name`, `doc`, `kind`, `args`, `fields`, `elements`, `members`, …, and `check`), `Kind`, `Field`, `Choice`; `setting(key)` reads the package's `strata.toml` while compiling, `compile_error` fails a `derive` with a message |
| `std.json` | `Json`: records and sum types read from JSON and written to it (`derive Json for Signup`; `Signup.from_json(parse(text)!)`, `stringify(s.to_json()!)`), and implemented for `Int`, `Float`, `Bool`, `String`, `List[T]`, `Option[T]` and `Dict[String, V]`, so `fn send[T: Json](x: T)` takes them; `JsonValue`, `parse`, `stringify` |
| `std.toml` | `Toml`: records and sum types read from TOML and written to it (`derive Toml for Config`; `Config.read_toml(text)`, `config.write_toml()`, `to_toml` / `from_toml` for `TomlValue`s), implemented for `Int`, `Float`, `Bool`, `String`, `List[T]`, `Dict[String, V]`, `Date`, `Time` and `DateTime`. TOML 1.1 read and written in Strata: `parse(text)` gives the document's root table (`List[(String, TomlValue)]`, keys in the order written) or an error saying where (`3:1: `b` is defined twice`); `TomlValue` is `Text`, `Integer`, `Number`, `Boolean`, `Moment` (a `DateTime`: optional `Date`, `Time` and `Offset`, TOML's four kinds), `Array` or `Table`; `lookup(doc, ["server", "port"])`; `stringify(doc)` writes TOML text |
| `std.http` | `Request`, `Response`, `Method`; `ok_html`, `ok_text`, `ok_json` (a value whose type has `Json`), `not_found`, `redirect`, …; `Router` (`app.get("/users/:id", handler)`, `req.param("id")`), for `strata serve` |
| `std.web` | `Html[M]` and `Attr[M]`: HTML as values, one function per element (`div`, `p`, `table`, `button`, …) and attribute (`class`, `href`, `on_click(msg)`, …); `render`, `document(title, head, body)` |
| `std.regex` | regular expressions written in Strata: `parse(pattern)` gives a `Regex` (`Literal`, `Class` of code-point ranges, `Look`, `Repeat`, `Capture`, `Concat`, `Alternation`) or an error; `show` prints the tree. `compile(r)` makes its Thompson NFA (`List[Inst]`), `is_full_match(prog, text)` runs it as a Pike VM, and `full_match(text, pattern)` does both: what `matches` means. `matcher(pattern)` makes a DFA (`Automaton`) when the pattern asserts nothing but `^` and `$`, else keeps the NFA; `run_matcher(m, text)` runs it. Compiled code uses them for every `matches` |
| `std.unicode` | sets of code points as `List[(Int, Int)]` ranges (`union`, `intersect`, `difference`, `negate`, `contains_code`), properties (`property("Greek")`, `property_value("gc", "Lu")`), simple case folding (`case_fold`); Unicode 16.0 tables in `std.unicode_tables` |
| `std.utf8` | text as UTF-8 bytes (`List[Int]`, each 0 to 255) and back: `encode(text)`, `decode(bytes)` / `decode_range(bytes, start, end)` (`None` if they aren't UTF-8) |

The **compiler written in Strata** is the `compiler/` package: a program, not part of std (programs don't import it). Its `main` runs one step of the front end on files (`strata --file compiler run -- lex FILE`, `parse`, `stored`, `check`, `eval`, …: see `compiler/main.strata`), and the tests compare what it prints with the Rust front end. Its modules:

| Module | What it has |
|---|---|
| `ast` | Strata's syntax tree as Strata types (`Decl`, `DeclKind`, `TypeExpr`, `Expr`, `Stmt`, `Pattern`, …), node for node as the compiler's |
| `binary` | `.strata` files read and written in Strata: `read(bytes)` (a module file or a bundle), `read_header`, `decode_module` (checking each definition's hash), `decode_definition`, `encode_definition` (a definition's bytes, exactly as strata writes them), `hash` / `interface_hash` / `code_hash`; `without_notes` (in `ast`). Its SHA-256 is private: it names definitions, and std has no public hash function until one has been reviewed as cryptography |
| `text` | the printer, in Strata: code as text, laid out as `src/printer.rs` lays it out (lines broken to stay within 80 columns): `module_text(decls)` (a module file's definitions), `decl_text`, `fn_decl_text`, `type_decl_text`, `expr_text(e, depth)`, `type_text`, `pattern_text`, `block_text`, `escape`. It prints a tree as given: `strata render` first turns a stored tree into its written form (names shortened to what the imports allow), which takes name resolution |
| `lex` | the lexer, in Strata: `lex(text)` gives the tokens (`Token { tok, line, col, nl_before, space_before }`, `tok` an `IntTok`, `StrTok`, `FStrTok`, `Ident`, `Sym`, …, then `Eof`) and the `//` comments, or the error `src/lexer.rs` gives (`1:9: unterminated string`); `token_text` / `comment_text` print them as `strata lex FILE` does |
| `parse` | the parser, in Strata: `parse_module(text, module)` / `parse_program(text)` give the definitions as `src/parser.rs` parses them (comments in place), or its error (`3:5: expected `)` (found Sym(","))`); `parse_expr(text)`. The tree is as written: storing it (`strata edit`) qualifies its references, which takes name resolution |
| `resolve` | names, in Strata, as `src/resolve.rs` does them: `stored(decls, std)` gives a parsed program (a script, or a package's modules together) in its stored form (every reference to a definition by its qualified name, locals and methods as written), with std's definitions in scope; `written(decls, std)` the written form `render` shows (each reference as short as the imports allow, a union written out named, a type one union lists shown inside it); `union_members` decides a union's bare members; `errors(decls, std)` gives the resolver's errors (a `use` of what doesn't exist or is private, a private name, std's host built-ins outside std, a mock outside tests, an extern called from another module) |
| `check` | the type checker in Strata, so far: `definition_errors` (a name defined twice, or the prelude's; two `impl Type` blocks; a trait implemented twice), `variant_errors`, `mixed_unions` (a union with members of both kinds split as the checker splits it), `expression_errors` (unknown variables, wrong numbers of arguments) and `type_errors` (inference for `Int`, `Float`, `Bool`, `String`, `()`, lists and the program's functions), each worded as `strata check` words it |
| `eval` | running a program from its stored trees, in Strata: `run(decls, args)` gives a `Run` (`stdout`, `stderr`, `code`), what the compiled program prints and its exit code, faults and their messages included. It finds methods from the receiver's type at run time and gives `main` the console and the process; `strata pack --prelude FILE` writes the prelude's definitions, which a program runs with. Not yet: files, mocks, `comptime` and `derive`, `extern`, and a type parameter's static methods (`T.zero()`, which take the type checker) |

**JSON** (`std.json`) is read into records and sum types that derive `Json`, in the form a web front end sends: a record is an object (with `"type"`, its type's name, when written; optional when read), a field left out takes its default, an `Option` is the value or `null` (or left out), a sum's variant is an object whose `"type"` names it (`{"type": "Circle", "r": 2}`, one positional field as `"value"`), a sum whose variants carry nothing is a string (`"Quiet"`), a tuple is an array, a `distinct` type is its underlying value, and a union is its member's value (reading takes the first member the JSON fits; members whose JSON overlaps are refused: `Int | Float`, two lists, `String` next to a choice, two members with the same `"type"`). Every refinement is checked, and errors say where: `home.zip: "1234" does not satisfy …`. Keys a record doesn't have are ignored, unless `strata.toml` says `unknown_keys = "error"` under `[json]`; `from_json_with(j, Reject)` or `from_json_with(j, Ignore)` decides for one call. Integers are within ±(2^53 − 1) both ways, the integers a JavaScript number holds exactly (`std.json.SafeInt`): reading or writing a larger one is an error (`to_json` returns a `Result`). A package that wants it proven instead sets `integers = "proven"` under `[json]`: then every integer field must be shown to be a `SafeInt` (typed so, or refined within it), and `strata verify` fails on one that may not be. A type with no such form (a function, `Option[Option[T]]`, a field named `type`, a variant with several positional fields) is an error at the `derive`, as is a field whose type doesn't derive `Json` itself. A generic type derives it for every type argument that has it (`derive Json for Pair` is `Pair[T]` for each `T: Json`). Through the generic impls, `Some(x)` where `x` is written as `null` (`Some(None)`) is an error from `to_json`; their elements' refinements are checked by `from_json` (`read[List[Port]]("[70000]")` is an `Err` naming `Port`). `print` shows any value as code.

```rust
from std.json use Json, parse, stringify

type Signup = { email: String where contains(it, "@"), age: Int where it >= 13, nickname: Option[String] }
derive Json for Signup

signup := Signup.from_json(parse(body)!)!
text := stringify(signup.to_json()!)  // {"type":"Signup","email":…,"age":…,"nickname":null}
```

**TOML** (`std.toml`) is read by a parser written in Strata, for `strata.toml` files once the tools read them in Strata. It checks every rule of TOML 1.1: keys and tables defined twice, tables closed once written (inline tables; tables made by dotted keys can't get a `[header]`, nor header tables dotted keys), integers within 64 bits, dates that exist (`2023-02-29` is an error), seconds that may be left out (`07:32`), inline tables over several lines with a trailing comma, `\e` and `\xHH` escapes. A Strata `Float` is finite, so `inf` and `nan` are errors. `tests/toml.rs` checks it reads documents as the `toml` crate does (the same values, the same documents refused), and that what `stringify` writes reads back as the same value; with `STRATA_TOML_CORPUS` set to a checkout of [toml-test](https://github.com/toml-lang/toml-test), on all of its TOML 1.1 files too.

```rust
from std.toml use parse, lookup, Integer

doc := parse("[server]\nport = 8080\n")!
port := match lookup(doc, ["server", "port"]) {
  Some(Integer(n)) => n,
  _ => 80,
}
```

`derive Toml` reads records and sum types from TOML as `derive Json` does from JSON, in the form a configuration file is written: a record is a table of its fields (no `type` key), a field left out takes its default, and an `Option` field is left out for `None` (TOML has no null, so an `Option` anywhere but a field is an error at the `derive`). A sum's variant is a table whose `type` names it (one positional field as `value`), a sum whose variants carry nothing is a string, a tuple is an array, a `Dict[String, V]` a table, and `Date`, `Time`, `DateTime` are TOML's dates and times. Every refinement is checked, errors say where (`servers[0].port: 70000 does not satisfy …`), integers are 64-bit, and keys a record doesn't have are ignored unless `strata.toml` says `unknown_keys = "error"` under `[toml]` (or `from_toml_with(v, Reject)`). The methods are named for TOML (`to_toml`, `from_toml`, `read_toml`, `write_toml`), so a type may derive `Json` too.

```rust
from std.toml use Toml

type Server = { host: String, port: Int where 0 < it && it < 65536 = 8080 }
type Config = { name: String, servers: List[Server] = [] }
derive Toml for Server
derive Toml for Config

config := Config.read_toml("name = \"app\"\n[[servers]]\nhost = \"a.example\"\n")!
text := config.write_toml()!
```

**Regular expressions** (`std.regex`) are written in Strata, so a self-hosted compiler can use them. `parse` reads the syntax of Rust's `regex` crate (Unicode by default: `\d`, `\w`, `\s` and `.` are Unicode's; `\p{Greek}`, `\p{gc=Lu}`, `\p{age=6.0}`; classes with `&&`, `--`, `~~` and `[[:alpha:]]`; named groups; the flags `imsUuRx`; `\b`, `\b{start}`, `\A`, `\z`; no look-around or back-references) into the tree regex-syntax builds: classes as ranges of code points with `(?i)` folded in, flags gone, the same simplifications (`ab|ac` is `a(?:b|c)`, `a|b` is `[ab]`). `tests/regex.rs` checks the two agree on curated and random patterns. The Unicode data (`std.unicode_tables`) is generated by `tools/unicode_tables.py` from regex-syntax's tables (Unicode 16.0.0).

```rust
from std.regex use parse, show

re := parse("(?i)ab|ac")!
show(re)    // cat(cls(41-41 61-61),cls(42-43 62-63))
```

**Web servers** (`std.http`) define `fn handle(req: Request) -> Response`, which `strata serve` calls for each request. Routes are written as with Hono or Express, on a `Router`:

```rust
fn app() -> Router {
  mut app := Router.new()
  app.get("/", req -> redirect("/users"))
  app.get("/users/:id", show_user)
  app
}

fn show_user(req: Request) -> Response {
  id := req.param("id") or { return not_found() }
  ok_text(f"user {id}")
}

fn handle(req: Request) -> Response {
  app().handle(req)
}
```

Routes are tried in order; `:name` segments are captured, and `req.param(name)` is an `Option` (a handler can be called without its route). A `HEAD` request uses the `GET` routes; no route for the path is a 404, and routes for it but not for its method a 405.

**Apps** follow the Elm architecture: a model, messages, and three functions that `strata serve` runs (see `examples/counter`):

```rust
type Model = { count: Int where 0 <= it && it <= 100 }

type Msg =
  | Increment
  | Reset

fn init() -> Model { Model { count: 0 } }

fn update(model: Model, msg: Msg) -> Model {
  match msg {
    Increment => Model { count: min(model.count + 1, 100) },
    Reset => Model { count: 0 },
  }
}

fn view(model: Model) -> Html[Msg] {
  div([], [text(f"{model.count}"), button([on_click(Increment)], [text("+")])])
}
```

`update` is a plain function: tests call it, and `strata verify` proves that every message keeps the model valid.

Events carry a message (`on_click(Reset)`), or make one from what the user entered: `on_input(s -> Typed { text: s })` for a field's value as it is typed, `on_change(make)` for a select or a checkbox (`"true"` / `"false"`), and `on_submit_form(fields -> Submitted { fields })` for a form's fields by name (a `Dict[String, String]`). Checking what was entered is ordinary code in `update`, so a refused form can keep its error in the model and show it next to the field (see `examples/todos`). `every(1500, Refresh)` sends a message on a timer while its element is on the page, `on_key("/", Search)` (or `key_bindings([...])`) when a key is pressed; `focused()` and `revealed()` focus an element or scroll it into view when they appear; `init` and `update` may ask for capabilities (`Files`) after their other parameters.

**HTML** (`std.web`) is built as values and rendered to text; see `examples/page`. Text is always escaped. Tag and attribute names are checked where they are written: `element` and `attr` take a `TagName` and an `AttrName`, and an attribute can't be an inline script (`attr("onclick", …)` is rejected when checking). `Html` and `Attr` are records around private types, so only those functions make them; the types themselves have nothing left to check, and the verifier passes over views. Events are attributes too: `on_click(msg)` carries a value of the page's message type `M`. The message isn't written into the page: `render` numbers the page's handlers (`data-handle-click="3"`), the page sends the number back, and `dispatch(page, n, value, fields)` gives handler `n`'s message (`strata serve` does this with the page it served, so a page can only send its own messages). A page without events is an `Html[()]`. The text inside a `<style>` element (`style_sheet(css)`) isn't escaped, but can't close the element.

Each capability is a trait, so code that prints takes a `Console`, and its signature says so. The real implementations are capabilities the runtime makes, for `main` and for a test that names one; nothing else can create them. Other tests pass the in-memory stand-ins (see `examples/wc`), or `mock Console`. Only `std` can call the native `host_*` operations underneath; `verify` and fuzzing never run code that reaches them.
