# The language

A guide to Strata: what you need to read and write it, feature by feature. The fine print (every rule, edge case and builtin) is in the [language reference](language-reference.md), section by section as here, and [Learn Strata in Y minutes](learn-strata.md) has the language in one valid file. Strata code is highlighted as Rust, the closest match.

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

fn ship(mut db: Db, id: OrderId, tracking: NonEmptyString)
  -> Result[Order, ShipError] where match it { Ok(o) => o is Shipped, Err(_) => true }
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
fn halve(n: Natural) -> Int where it >= 0 ensures it * 2 <= n { n / 2 }
```

The prelude provides the common ones: `Natural`, `Positive`, `Negative`, `NonZero`, `Probability`, `Percentage`, `NonEmpty[T]`, `NonEmptyString`. `strata builtins` lists them with every builtin function.

A bare alias adds nothing, so `type Name = String` is an error. Constrain it with `where`, or make it a separate nominal type:

```rust
distinct type UserId = String where matches(it, "u_[0-9]+")
id := UserId("u_42")      // explicit construction, checked
id.value                     // the underlying String
```

A `distinct` type never mixes with its base or with other distinct types (`OrderId` ≠ `UserId`), it can have trait implementations, and it serialises to JSON as its underlying value.

## Refinements and contracts: one way each

A refinement says what one value is; a contract (`requires`, `ensures`) relates values. Each fact has one place:

- a `requires` (or a conjunct of one: `a && b` is two) that names one parameter, and constants, is that parameter's refinement: `requires price >= 100` is `price: Cents where it >= 100`;
- an `ensures` that names only `it` is the result's refinement: `ensures it >= 0` is `-> Cents where it >= 0`; one that names only an element of a tuple result (`it.1`, or a named result) is that element's: `-> (next: Q, added: Int where it >= 0)`;
- a refinement names only `it` and constants: one that names another parameter (`k: Int where it <= n`) is a contract, `requires k <= n` (on the result, an `ensures`).

`strata check` (and `strata edit`, for what it wrote) warns on each one written the other way, showing the replacement, and `strata fix` makes it (`strata fix NAME` for some definitions). A `mut` parameter keeps its `requires`: a refinement holds for the variable's whole life, a `requires` only on entry. A trait's methods, and an implementation's of a trait, are left as they are: the trait sets their signatures.

**Named results.** A function returning a tuple can name its elements, for its readers and for its `ensures`, where the names are bound to the elements (`it` is still the whole tuple). Callers destructure the tuple as before:

```rust
pub fn mem_enqueue[P, E](q: MemQueue[P, E], tasks: List[Task[P]]) -> (next: MemQueue[P, E], added: Natural)
  ensures added <= len(tasks)
  ensures len(next.entries) == len(q.entries) + added
{ ... }

(q2, n) := mem_enqueue(q, tasks)
```

**`old`: a value before the call.** A function can change its `mut` parameters (and a method `mut self`), so its `ensures` may relate what they are when it returns to what they were when it started: `old e` is `e` evaluated where the function started. In an `ensures`, a `mut` parameter's name is its value at the end:

```rust
fn push_one(mut xs: List[Int], x: Int)
  ensures len(xs) == old len(xs) + 1 {
  xs += [x]
}

trait Counter {
  fn total(self) -> Int
  fn incr(mut self)
    ensures self.total() == old self.total() + 1
}
```

`old` is a prefix keyword that binds tighter than any operator: `old a + b` is `(old a) + b`, and it takes a whole chain of fields and calls (`old q.entries.get(id)`); parentheses only group (`old (a + b)`, `old(x)` is `old x`). Its value has the type of `e`. It goes only in an `ensures`, once (`old old e` is an error), over what the function starts with: its parameters, not `it` nor a name bound inside the clause (a lambda's parameter). `old e` where `e` names no `mut` parameter is the same value before and after: `strata check` warns, and `strata fix` drops the `old`. Callers rely on the contract with the arguments' values before and after the call, as `verify` sees it (a generic function calling a trait's `mut self` method too); a broken one shows both values (`before: len(xs) = 2, after: len(xs) = 2`). Outside an `ensures`, `old` is an ordinary name (`old := x`).

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

A constructor that leaves out a field with a default gets the default; so does JSON input with the key missing, and a command line without the option. Nothing gets a default implicitly.

### Spreading a record into a call

`f(...x)` gives the fields of record `x` to the parameters of `f` with the same names; fields `f` doesn't take are left alone:

```rust
type Order = { id: Int, qty: Int, price: Int, note: String }
type Fee = { fee: Int }

fn total(qty: Int, price: Int) -> Int { qty * price }
fn with_fee(id: Int, qty: Int, price: Int, fee: Int) -> Int { qty * price + fee }

total(...order)                     // total(order.qty, order.price)
with_fee(...order, 2)               // with_fee(order.id, order.qty, order.price, 2)
with_fee(...order, ...Fee { fee: 2 })
shop.line(...order)                 // methods, and `Shop.new(...x)`, alike
```

It is the call written out, decided when the program is checked, so it runs, compiles and is verified as that call. Positional arguments come before the spreads (they give the first parameters) or after them (the last); the spreads give the rest, each parameter from the one record that has a field of its name. A parameter no record gives, or two give, is an error, as is a record that gives none, and a `mut` parameter, which is passed explicitly (`mut x`). A value spread that isn't a variable or a field of one is computed once, before the call's other arguments.

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

**A member it declares can have a `///` doc comment**, written above its `|`, as a record's field can: `std.typing` gives it to derived traits (a `CommandLine` union shows it as the command's help).

```rust
type Command =
  /// Show the history.
  | Log
  /// Replace definitions, read from standard input.
  | Edit { module: Option[String] }
```

**Only bare members flatten.** `type S2 = S | C` with `type S = A | B` is `A | B | C`; `type S3 = D(S) | E` is `D | E`: `D` is a new type holding an `S`.

**Patterns are sets of values**, and an arm takes what the arms above it left:

- a type's name: `Positive =>`, `String =>`, `List[Int] =>`, `Circle =>` (a variant, whatever its fields), or a trait (`Named =>`, values that implement it);
- a variant or record with fields, as before: `Some(x)`, `Pending { id, .. }`;
- a literal: `0`, `"x"`, `true`;
- a condition on the value `it`: `where 3 <= it && it <= 5 =>`, `where len(it) > 0 =>`, `where it < 0.5 =>`;
- alternatives: `Square | Triangle =>`, `0 | 1 | 2 =>`, `(0, x) | (x, 0) =>` (they bind the same names);
- `_`.

**Narrowing.** Inside an arm, the matched variable has the arm's type (`s` is a `Square` above), and no new name is needed. To match the result of an expression, name it: `match lines(text) as ls { … }`. A matched field (`match t.status { Closed => t.status.why, … }`) narrows too. Assigning to a narrowed `mut` variable ends its narrowing.

**Exhaustive and reachable.** The checker reports a value no arm takes (`` `where 3 <= it && it <= 5` is not covered``), and an arm the arms above already cover (`unreachable match arm `Positive``). The matched value's own refinement counts: in `fn f(n: Positive)`, `match n { 1 => …, where it > 1 => … }` is exhaustive.

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
- **Laws** are checked by `strata verify` against every implementation: proven by the solver, which sees through the implementation's methods (when they end and can't fail), else with generated inputs. In `examples/traits.strata`, both laws are proven for `Money` and for `List[T]`, and the solver finds that averaging (`Midpoint`) is not associative.
- **Generic functions** are fuzzed with a type that actually implements their bounds.

**Capabilities are traits too.** There are no separate effects: a backend (database, queue, console, clock) is a trait, and code that uses one takes an implementation as a parameter or holds it in a field: `fn ship(mut db: Db, ...)`, `type PrintWorker = { console: Console }`. A trait used as a type means "any implementation": `db: Db` is shorthand for `fn ship[D: Db](db: D, ...)`. Name the type parameter only when it must tie things together, as in `combine_all[T: Combine](first: T, rest: List[T]) -> T`. A method that changes its value says `mut self` (`fn save(mut self, o: Order)`). Tests pass a `mock Db`. Iterators and tasks are language features of their own, not capabilities (see Iterators, and Tasks).

**Mocks, for tests.** A mock is declared like a type, with `mock` in place of `type`, and implemented like any type; its implementations may leave methods out:

```rust
/// An in-memory disk, for tests.
mock MemDisk = { files: Dict[String, String] = Dict[String, String] {} }

impl Files for MemDisk {
  fn read(self, path: String) -> Result[String, FsError] { Ok(get(self.files, path) ?? "") }
  fn write(mut self, path: String, text: String) -> Result[(), FsError] {
    mut self.files.insert(path, text)
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
- A mock is for tests only: a `test`, another mock, or an `impl` for one may name it; anything else is an error, so a mock never reaches a real run.

**Capabilities given by the runtime.** A trait that reaches the outside world is marked `capability trait`; anything may implement it, mocks for tests included. The real thing is a `capability`: functions that reach the outside world, holding no data. Only the runtime makes one, for `main` or a test that asks for it by name, and its functions are called on that value, so holding it is the permission:

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

/// Any clock: the system's, or a mock in a test.
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
- A test may ask for capabilities the same way (`test "..." (sys: SystemClock) { ... }`); one that doesn't uses mocks.
- `verify` never runs a capability's code (it reports it as assumed), nor code that takes one; that code is proven from the capability's contracts like any other.

std's are capabilities too: `Disk` (`std.fs`, for `Files`), and `Process` and `Launcher` (`std.process`, for `Environment` and `Commands`: running other programs), which `main` names. `Console` is the one capability the environment chooses, so `main` asks for the trait: it gets the terminal under `strata run`.

**A type's own methods.** `impl Item { ... }` gives a type methods of its own, with no trait. A method with `self` is called on a value (`item.priced(42)`). One without `self` is called on the type, which makes it a constructor: `Item.new("tea")`. Only a record, sum or `distinct` type defined in the program can have them, in the module that defines it, and in a single `impl Item` block.

```rust
impl Item {
  fn new(name: NonEmptyString) -> Item { Item { name, cents: 0 } }
  fn priced(self, cents: Natural) -> Item { Item { ..self, cents } }
}
tea := Item.new("tea").priced(42)
```

**Trait methods without `self`** are called on a type, never on a value: `Money.empty()`, `T.empty()` in a generic function, `Self.empty()` in a trait's default methods and laws. They are constructors and the like (`fn empty() -> Self`, `fn parse(s: String) -> Result[Self, ParseError]`), so the signature must mention `Self`.

```rust
trait Empty: Combine {
  fn empty() -> Self
  law identity(a: Self): Self.empty().combine(a) == a
}
fn combine_any[T: Empty](xs: List[T]) -> T { fold(xs, T.empty(), (acc, x) -> acc.combine(x)) }
```

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

Procedural code is normal Strata: `loop { … }` repeats until `break`, and `continue` skips to the next iteration. A loop can state what stays true from pass to pass, between its header and its body: `while i < n` then `invariant 2 * total == i * (i + 1)` on the next line. It is checked at the loop's head when the program runs, and `strata verify` proves it (see [verification](verification.md#loops)); `ghost invariant` (and `ghost requires`, `ghost ensures`) leaves a clause to `verify` alone, never checked when the program runs ([ghost contracts](verification.md#ghost-contracts-proven-never-checked)); most loops need none, as `verify` infers simple ones. A `while` or `loop` can also say why it ends, the same way: `decreases n - i`, or `decreases (left, size)` for measures compared in order (one goes down, or it stays and the next does); a recursive function says it beside `requires` (`decreases (m, n)`). It is proven by `verify` and checked when the program runs where it isn't (see [termination](verification-reference.md)); `verify` finds most measures on its own. Method chains can be split across lines:

```rust
best := scores
  .filter(s -> s > 0)
  .sorted()
  .reversed()
  .take(3)
```

**Bindings.** `:=` names a new value and infers its type: `n := 0`, `(w, k) := pair()`. With a type, the colon is already there, so the sign is `=`: `let x: Int = 5`. A mutable variable is `mut x := 0` or `mut x: Int = 0`. A plain `=` changes an existing variable or field (`x = e`, `line.qty = …`), and `+=`, `-=`, `*=`, `/=`, `%=` are short for `x = x + e` and so on (and are how such an assignment is printed). A function that returns `()` leaves out `-> ()`: `fn log(s: String) { … }`. A call's parentheses are written against the name: `f(x)` is a call, while `f (x)` is `f` followed by `(x)`.

Variables are immutable unless declared `mut`. `while` and `for` loops are available. Mutation is always *visible*:
- a function that changes an argument declares it `mut`, and the caller passes a mutable variable marked `mut` at the call (`restock(mut line, 5)`), which is updated when the call returns. The marker is required there, and an error anywhere else;
- a method that changes its receiver takes `mut self`, and the call is marked the same way: `mut queue.claim(2)` (as Hylo writes `&x.append(1)`). The mark is required for such a method, and an error on any other. It marks the first call on the place after it: `mut queue.claim(2).len()` changes `queue` through `claim`, and is written `(mut queue.claim(2)).len()`. As an argument, the marked call is put in parentheses, `f((mut queue.pop()), x)`, to tell it from `f(mut queue)`, which passes `queue` to a `mut` parameter.

There are no shared references, so nothing changes behind your back: `ys := xs` is a copy, and changing `xs` afterwards leaves `ys` as it was (the same holds for passing a value or storing it in a field). There is no `copy` or `clone` to call; the copying is the compiler's business (compiled code shares the value and copies it only when one side changes it while the other still holds it: see [values and copying](#values-and-copying) for what that costs). A declared type holds for the variable's whole life: every assignment is checked against it.

**Ranges, slices and comprehensions.** `0..n` is the integers from `0` to `n - 1`, and `1..=n` includes `n`; a range is a `List[Int]` (`for i in 0..n { … }`, `(0..n).map(…)`). A list is sliced with a range: `xs[1..3]`, `xs[..3]`, `xs[2..]`, `xs[..=i]`, clamped to its length as in Python. One element is `xs[i]`: `i` must be in range, which is checked when the code runs and proven by `verify`; `get(xs, i)` gives an `Option` instead. An element of a `mut` list is assigned the same way, `xs[i] = v` (and `xs[i] += 1`, `grid[r][c] = v`, `s.items[i] = v`, `s.items[i].qty = 3`): values never change, so it means `xs = set_at(xs, i, v)`, `xs` with element `i` replaced (a copy held elsewhere keeps the old one), with the same condition on `i`. It is O(1): the compiled code changes the list in place when nothing else holds it. A comprehension builds a list from one or more others, with a `where` to keep only some elements, as in a refinement:

```rust
[x * x for x in xs where x > 0]
[(a, b) for a in 1..=3 for b in ["x", "y"] where a != 2]
[(name, n) for (n, name) in pairs]
Dict { w: len(w) for w in words }
Set { x % 3 for x in xs }
```

Lambdas are written with an arrow: `x -> x + 1`, `(acc, x) -> acc + x`, `() -> 7`, with optional parameter types (`(s: String) -> s + "!"`; a refined one, such as `(n: Positive) -> …`, is checked on every call, like a named function's) and a block for a longer body (`x -> { ... }`). `return e` leaves the enclosing function or lambda early. A lambda of several parameters where a function of one tuple of as many elements is expected takes the tuple apart: `pairs.filter((k, v) -> v > 0)`, `xs.enumerate().map((i, x) -> i * x)` bind the elements, with the tuple's types (written without parameter types; a lambda passed where a function of two arguments is expected, as `fold`'s, stays one of two arguments). A parameter, a lambda's or a function's, can also be a pattern that always matches (a tuple, a record pattern, nested, `_`), with or without a type: `pairs.map(((a, b)) -> a + b)`, `fold(m, 0, (acc, (k, v)) -> acc + v)`, `((a, b): (Int, Int)) -> a * b`, `fn dist((x, y): (Int, Int)) -> Int`, `(Seg { from: Pt { x, y }, to }) -> …`. It means taking the argument and binding it with the pattern, as `:=` does; a pattern that may not match (a variant, a literal) is refused: take the value and `match` on it.
### Unused values and variables

Values never change, so a statement whose value is dropped almost always means a mistake: `xs + [1]` on its own line makes a new list and throws it away. An expression statement whose value isn't `()` is an error (`the value of `xs + [1]` is not used: use it, or discard it with `_ := …``), and a value dropped on purpose is written `_ := e`. That covers a call kept only for what it does through a capability when it also returns something, an `if` or `match` statement whose arms give values (discard the arms' values, or the whole), and `x + 1`; a `mut` method call returning `()` (`mut q.claim(2)`), a `spawn` (its task belongs to the `scope`) and a `return`/`break` are fine.

A variable never read (a parameter, `x := e`, `mut x := e`, a binding of a pattern, a lambda's parameter) is a warning, not an error, since code may be partway written: one line each from `check`, `run`, `build` (when it verifies) and `edit`, naming the function. A name starting with `_` (`_x`, `_`) says the value is unused on purpose; an `impl` method's parameters, which its trait or type dictates, are not warned about. A variable whose value has no effect (a literal, a variable, a lambda, a field, a tuple or list of them) and that nothing reads isn't compiled at all.

Lambdas are written with an arrow: `x -> x + 1`, `(acc, x) -> acc + x`, `() -> 7`, with optional parameter types (`(s: String) -> s + "!"`; a refined one, such as `(n: Positive) -> …`, is checked on every call, like a named function's) and a block for a longer body (`x -> { ... }`). `return e` leaves the enclosing function or lambda early. A lambda of several parameters where a function of one tuple of as many elements is expected takes the tuple apart: `pairs.filter((k, v) -> v > 0)`, `xs.enumerate().map((i, x) -> i * x)` bind the elements, with the tuple's types (written without parameter types; a lambda passed where a function of two arguments is expected, as `fold`'s, stays one of two arguments).

## Values and copying

Values never change: `ys := xs + [x]` is a new list, and `xs` is as it was; a variable declared `mut` changes, by assignment or by a `mut` method (`mut d.insert(k, v)`), and a copy held elsewhere keeps the old value. What makes that cheap is the rule compiled code follows: **an update of a value held by one thing only is made in place; a value something else still holds is copied first.** The copy is then held by one thing only, so the updates after it are in place again: sharing a value costs at most one copy per update that follows a share, never a copy per update.

An update is in place (when nothing else holds the value) in these forms:

- `mut d.insert(k, v)`, `mut d.remove(k)` on a dict or a set, where `d` is a variable or a field at any depth (`mut s.d.insert(k, v)`, `mut b.bids.words.remove(k)`): O(1) each, on average.
- `xs[i] = v` (`xs = set_at(xs, i, v)`) on a list: O(1).
- `x.f = v` and `x.a.b = v`: a field of a record or tuple, at any depth (not of a `distinct` record or one with a refinement of its own, which is checked on the new record).
- `mut xs.push(e)`, `mut xs.pop()`, `xs += ys`, `xs = xs + ys`, `xs = xs + [e]` on a list: O(1) per element added, on average, **even when `xs` is shared**: the list's buffer is extended past its end when no other list made from it has used those slots, and copied otherwise (with room for as much again, so a list grown over and over is copied O(log n) times).
- `s += t`, `s = s + t` on a string: in place when the string is held by `s` only (and the records on the way to it, for a field), else a copy, with room to grow.
- A record's or a container's elements of a refined type (`Dict[Int, Word]`): an update checks what it adds (`insert`'s key and value, `push`'s element), not every element again.

Any other expression makes a new value and leaves its operands as they were: `ys := xs + [x]` copies `xs` (it is still `xs`'s), and so do `xs.sorted()` and `xs.reversed()`. Change the place itself (`mut xs.push(x)`, `mut xs.sort()`) to update it in place.

What holds a value (so that changing it through one of them copies it):

- each variable, field, and container element it is stored in: `ys := xs` holds it twice, and the next `mut xs.insert(…)` copies (once);
- a `mut` parameter is the caller's variable, by reference: `fn add(mut s: State)` changing `s.d` changes the caller's value in place, as does a `mut self` method (`mut c.add(1)`). A field given to a `mut` parameter, `bump(mut b.bids)`, is lent to the call: in place when the records on its path are held once and no other argument of the call reads `b`;
- a value read out of a container is held by the container too: `mut inner := get(outer, k) ?? Dict {}` then `mut inner.insert(k2, v)` copies the inner dict every time (each step's copy is as big as the inner dict). Take it out first: `mut outer.remove(k)` after reading it, then change `inner` and put it back (`mut outer.insert(k, inner)`) (a list read out is fine to `push` to: see above);
- a `for` loop holds what it runs over until it ends: changing the container inside the loop copies it once;
- a closure holds what it captures, for as long as it exists;
- a function's (not `mut`) parameter is still the caller's value: `mut e := d` then `mut e.insert(…)` copies `d` once;
- a value sent to a task or a channel is held by the sender too while it still uses it.

`strata run --copies` (and `strata test --copies`, `strata build --copies`) reports each copy made to change a value, when the program ends: per place in the code, how many, how big, and why (on standard error, the most bytes first):

```
copies: 2 copies, 14 kB, at 1 place (the most bytes first):
  copied a Dict of up to 400 entries at app.kept@31f7e4cc/body/4/body/0/value: 2 times, 14 kB (shared: held 2 times)
```

The place is a location, as `strata find` and `strata refs` print them: `strata view app.kept@31f7e4cc/body/4/body/0/value` shows it. With nothing copied it says `copies: none`. Without `--copies` the compiled code is the same as ever (nothing is counted).

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
- `e except { fallback }` handles a failed `Result` here: the `Ok` value, or else the block. `except err { ... }` names the error. The fallback must produce the same type, or leave with `return` (or, in a loop, `break` / `continue`: `x := opt or { continue }`): a block that ends in one never gives a value, so the binding gets the value's type.
- `e or { fallback }` does the same for an `Option`: the `Some` value, or else the block (`None` has nothing to name).
- `e !! value` and `e ?? value` are the short forms, for a fallback that is a single value on one line; such a fallback is always written that way. They bind tighter than comparisons and looser than arithmetic (`n ?? 0 + 1` is `n ?? (0 + 1)`, `n ?? 0 == 1` is `(n ?? 0) == 1`), and chain: `a ?? b ?? 0`.
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

A fault in a task ends the program, as anywhere else (see [supervised tasks](#supervised-tasks) for the exception), and a deadlock, every task waiting with none able to go on, is reported as a panic instead of hanging.

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
- **Closing is explicit, once per task.** Each task holding the sender calls `tx.close()` when it is done sending; the channel ends when all of them have.

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

A cancelled task stops at its next wait (a `recv`, a `send`, an `await`, a `sleep`, …), however deep in calls it is, and leaves as `return` would.

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

## Built-in types

Every program has these without a `use`:

| Type | Notes |
|---|---|
| `Int`, `Float`, `Bool`, `String` | `Int` never overflows: a machine integer while it fits, a big integer when it doesn't (like Python). `Float` is never NaN or infinite. There is no implicit Int/Float mixing (`float(n)`, `round(x)`). Integer `/` and `%` are Euclidean: the remainder is never negative (`-7 / 2 == -4`, `-7 % 3 == 2`, `7 % -3 == 1`); for C's truncation, divide the magnitudes and put the sign back (`-(7 / 2) == -3`). `parse_int` takes an optional sign, `+` or `-` (`parse_int("+5") == Some(5)`): check the first character when only digits are wanted. Strings are UTF-8, and ordered (`<`, `<=`, `>`, `>=`, `compare`, `sort`) by their code points, which is UTF-8's byte order |
| `I8` … `I64`, `U8` … `U64` | sized integers, for code that needs a fixed width: their arithmetic wraps around, and they never mix with `Int` (`x.to_int()`, `n.to_u32()`) |
| `()` | the unit type: "no meaningful value", Python's `None` as a *type* |
| `Option[T]` | `None` or `Some(x)`: a value that may be absent |
| `Result[T, E]` | `Ok(x)` or `Err(e)`: an operation that may fail |
| `List[T]` | an ordered sequence, `[1, 2, 3]` |
| `(A, B, …)` | tuples, `(word, 3)`, destructured with `(w, n) := …`, accessed with `t.0` |
| `Dict[K, V]` | `Dict { "a": 1, "b": 2 }`, `get`, `insert`, `remove`, `keys`, `values`, `items` |
| `Set[T]` | `Set { 1, 2 }`, `insert`, `union`, `intersection`, `difference` |

All values are immutable (`insert` returns a new dict). `+` adds numbers and joins strings and lists. A string's unit is always named: `s.bytes()`, `s.code_points()` or `s.graphemes()`. **f-strings** interpolate any expression, with Python's format specs: `f"{name} has {len(xs)} items"`, `{price:.2}`, `{n:>5}`. Every value can be shown: `debug(x)` writes it as code, and `to_string(x)` (what `print` and f-strings use) is a string's text and anything else's `debug`; a type changes either with an `impl` of `Debug` or `Display`. `strata builtins` lists every builtin function with its signature.

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

Every iterator has `map`, `filter` and `enumerate` (each element with its index, `(i, x)`), which are lazy. One that ends implements `FiniteIterator` too, and `verify` shows that a `for` loop over it ends. `xs.iter()` gives one over a list, a set or a dict, and a string's units are iterators.

A finite iterator becomes a list where a `List` is expected and where it is shown, as if `to_list(...)` were written: passed for a `List` parameter (`count(s.graphemes())`), compared with a list (`s.graphemes() == ["a", "b"]`), printed, interpolated or given to `to_string` (`print(s.graphemes()[1..3])` prints `["b", "c"]`). An iterator that may not end can't become a list: that is an error, saying to take a part of it first (`.take(n)`). Each conversion walks the iterator once, O(n).

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

**Command-line options** are a record that derives `CommandLine` (`std.cli`): each field is read from the arguments by its type (a `Bool` is a flag, a `List[T]` the remaining arguments, a field with a default an option), every refinement is checked, and `--help` is built from the `///` doc comments. A union that derives `CommandLine` is a set of commands (`Edit` is `edit`), each with its own options.

`main` can also ask for `Console` and capabilities (`Disk`, `Process`, the program's own), in any order, and passes them on where a trait (`Files`, `Environment`) is wanted, so tests can pass stand-ins. It returns `()`, an `ExitCode`, or a `Result` of either: an `Err` is written to standard error and the exit code is 1.

The **standard library** is the `std/` package, written in Strata and built into `strata`, so any program can `use std.…` without declaring a dependency. It has the console, files, time and processes (`std.io`, `std.fs`, `std.time`, `std.process`), command lines (`std.cli`), sorted dictionaries and sets and a priority queue (`std.collections`), JSON and TOML (`std.json`, `std.toml`), HTTP and HTML as values (`std.http`, `std.web`), and regular expressions and Unicode (`std.regex`, `std.unicode`). Each module has a [reference page](std/README.md), generated from its doc comments by `strata doc`.

**JSON**: a type that derives `Json` is read from JSON and written to it, every refinement checked:

```rust
from std.json use Json, parse, stringify

type Signup = { email: String where contains(it, "@"), age: Int where it >= 13, nickname: Option[String] }
derive Json for Signup

signup := Signup.from_json(parse(body)!)!
text := stringify(signup.to_json()!)  // {"type":"Signup","email":…,"age":…,"nickname":null}
```

**C functions** are declared with `extern c fn`, a signature with contracts and no body; see the [reference](language-reference.md#foreign-functions-extern-c).
