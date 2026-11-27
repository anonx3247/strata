# The language

This guide covers what you need to read and write Strata, one feature at a time. The [language reference](language-reference.md) has every rule, edge case and builtin, in sections that follow the same order. [Learn Strata in Y minutes](learn-strata.md) shows the whole language in one valid file. Code blocks are highlighted as Rust, the closest match.

## A taste

`examples/bank.strata` is the smallest complete example. `examples/orders.strata` is a larger one, excerpted below. `examples/tasks/` is a port of a real library, with a write-up of what verification found in it.

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
  model orders: Dict[OrderId, Order]     // what any store holds; contracts are written over it

  fn load(self, id: OrderId) -> Option[Order]
    ensures it == self.orders.get(id)
  fn save(mut self, id: OrderId, o: Order)        // `mut self`: saving changes the store
    ensures self.orders == (old self.orders).with(id, o)
}

fn ship(mut db: Db, id: OrderId, tracking: NonEmptyString) -> Result[Order, ShipError]
  ensures match old db.orders.get(id) {
    Some(Pending { lines, .. }) => it == Ok(Shipped { id, lines, tracking })
      && db.orders == (old db.orders).with(id, Shipped { id, lines, tracking }),
    Some(_) => it == Err(NotPending) && db.orders == old db.orders,
    None => it == Err(NotFound) && db.orders == old db.orders,
  }
{
  order := db.load(id) or { return Err(NotFound) }
  match order {
    Pending { lines, .. } => {
      o := Shipped { id, lines, tracking }
      mut db.save(id, o)
      Ok(o)
    },
    _ => Err(NotPending),
  }
}

fn refund(o: Order) -> Cents
  = ?refund_policy          // a typed hole: callers still check
```

## Refinements: named, inline, and structural

A refinement type is a base type plus a predicate. `type Quantity = Int where 1 <= it && it <= 99` is still an `Int`, so any `Int` can be passed where a `Quantity` is expected, without a cast. The predicate is checked when a value becomes a `Quantity`: at compile time for literals, at run time for other values, and for every input by `strata verify`.

A refinement used once can be written inline:

```rust
fn pick(level: Int where 5 <= it && it <= 13) -> Positive { level - 4 }
fn halve(n: Natural) -> Int where it >= 0 ensures it * 2 <= n { n / 2 }
```

The prelude defines the common ones: `Natural`, `Positive`, `Negative`, `NonZero`, `Probability`, `Percentage`, `NonEmpty[T]` and `NonEmptyString`. `strata builtins` lists them along with every builtin function.

An alias with no predicate adds nothing, so `type Name = String` is an error. Add a `where`, or declare a separate nominal type with `distinct`:

```rust
distinct type UserId = String where matches(it, "u_[0-9]+")
id := UserId("u_42")      // explicit construction, checked
id.value                     // the underlying String
```

A `distinct` type never mixes with its base type or with other distinct types (`OrderId` ≠ `UserId`). It can implement traits, and it serialises to JSON as its underlying value.

## Refinements and contracts: one way each

A refinement constrains one value. A contract (`requires`, `ensures`) relates several values. Each fact has exactly one place to be written:

- A `requires` that names one parameter and constants belongs in that parameter's refinement. `requires price >= 100` is written `price: Cents where it >= 100`. Each conjunct counts separately: `a && b` is two facts.
- An `ensures` that names only `it` belongs in the result's refinement. `ensures it >= 0` is written `-> Cents where it >= 0`. If it names only one element of a tuple result (`it.1`, or a named result), it belongs on that element: `-> (next: Q, added: Int where it >= 0)`.
- A refinement may name only `it` and constants. One that names another parameter (`k: Int where it <= n`) is a contract: `requires k <= n`, or an `ensures` when it is on the result.

`strata check` warns about each fact written in the wrong form and shows the replacement. `strata edit` does the same for what it writes. `strata fix` applies the replacements, to every definition or to the ones named (`strata fix NAME`). Two cases are left alone:

- A `mut` parameter keeps its `requires`, because a refinement holds for the variable's whole life and a `requires` only on entry.
- Trait methods and their implementations keep their form, because the trait sets their signatures.

**Named results.** A function that returns a tuple can name the elements. The names are bound in its `ensures`, and `it` is still the whole tuple. Callers destructure the tuple as usual:

```rust
pub fn mem_enqueue[P, E](q: MemQueue[P, E], tasks: List[Task[P]]) -> (next: MemQueue[P, E], added: Natural)
  ensures added <= len(tasks)
  ensures len(next.entries) == len(q.entries) + added
{ ... }

(q2, n) := mem_enqueue(q, tasks)
```

**`old`: a value before the call.** A function can change its `mut` parameters, and a method can change `mut self`. Its `ensures` can then relate their final values to their initial ones. In an `ensures`, a `mut` parameter's name means its value at the end, and `old e` means `e` evaluated when the function started:

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

Rules for `old`:

- It is a prefix keyword that binds tighter than any operator: `old a + b` is `(old a) + b`. It takes a whole chain of fields and calls (`old q.entries.get(id)`). Parentheses only group: `old (a + b)`, and `old(x)` is `old x`.
- `old e` has the type of `e`.
- It is allowed only in an `ensures`, at most once per expression (`old old e` is an error), and only over the function's parameters. It cannot refer to `it` or to a name bound inside the clause, such as a lambda's parameter.
- If `e` names no `mut` parameter, `old e` equals `e`. `strata check` warns, and `strata fix` removes the `old`.
- Outside an `ensures`, `old` is an ordinary name (`old := x`).

Callers rely on the contract using the argument values before and after the call. `verify` does the same, including in a generic function that calls a trait's `mut self` method. A broken contract shows both values: `before: len(xs) = 2, after: len(xs) = 2`.

**What a function or method says of its result's variant narrows its calls.** A result type `-> Order where it is Shipped`, or an `ensures` saying which variant the result is, gives the call that type where it is made:

```rust
fn ship(o: Order) -> Order
  ensures (it is Shipped) == (o is Paid || o is Shipped) { ... }

fn label(o: Order) -> String {
  match o {
    Paid => ship(o).tracking,               // a `Shipped`: no match needed
    Pending => match ship(o) {              // not a `Shipped`: two arms cover it
      Pending => "pending",
      Paid => "paid",
    },
    Shipped => ship(o).tracking,
  }
}
```

- `strata check` narrows a call when its function's facts about `it` are `it is V` (with `!`, `&&`, `||`), `A ==> it is V`, or `(it is V) == A`, and the arguments' types decide `A`: `o is Paid` where `o` is narrowed to `Paid` (by a `match` arm or an `if o is Paid`). The narrowed result can read a field only those variants have, and a `match` on the call covers only them; an arm for a variant ruled out is an error.
- When `A` is something else (`d.contains(k)`, a comparison), the checker leaves the match's coverage to `verify`, which proves that some arm matches from the callee's contract and the facts at the call (`if d.contains(k) { match lookup(d, k) { Some(v) => v } }`). Until it is proven, the compiled match faults with `no match arm` for a value no arm takes.
- Methods narrow the same way, `self` being the receiver; a trait method's facts hold of every implementation. `Option`, `Result` and other generic sums keep their type arguments when narrowed (`match same(o) { Some(v) => v }` where `o` is a `Some`).
- A field read of such a call (`find(d, k).tracking`) under a condition only `verify` decides reads the field of the variants left that have it. `verify` proves the result is one of them (`field `tracking` of a call's result`), and until it is proven the compiled code checks it there. A variable bound from such a call keeps the strict rule.
- The callee's contract is taken as given, as for every call. `verify` proves it in the callee; while it isn't proven, the callee checks it when it returns, so the caller never sees another variant.

## Records and defaults

A record field can have a default, written after its type:

```rust
type Retry = {
  attempts: Int where 1 <= it && it <= 10 = 3,
  backoff: List[Int] = [100, 200],
  label: String = "retry",
}
r := Retry { attempts: 5 }      // backoff: [100, 200], label: "retry"
d := Retry {}
```

A missing field takes its default in a constructor, in JSON input, and on a command line. Fields without a declared default have none.

### Spreading a record into a call

`f(...x)` passes the fields of record `x` to the parameters of `f` that have the same names. Fields that `f` doesn't take are ignored:

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

The spread is expanded into the written-out call when the program is checked, so it runs, compiles and verifies exactly as that call would.

- Positional arguments before the spreads fill the first parameters, and positional arguments after them fill the last ones. The spreads fill the rest.
- Each remaining parameter must come from exactly one record. A parameter that no record provides, or that two records provide, is an error. So is a record that provides nothing.
- A `mut` parameter cannot come from a spread. Pass it explicitly (`mut x`).
- A spread value that isn't a variable or a field of one is evaluated once, before the call's other arguments.

### Structural types: any record with these fields

`{ id: OrderId, total: Cents, .. }` is any record that has at least these fields, with these types. The `..` stands for the other fields a record may have, and must be written. It is a parameter's type, or a type parameter's bound:

```rust
type Invoice = { id: OrderId, total: Cents, paid: Bool }
type Quote = { id: OrderId, total: Cents, days: Int }

fn due(x: { id: OrderId, total: Cents, .. }) -> Cents
  ensures it <= x.total
= discount(x.total)

fn larger[T: { total: Cents, .. }](a: T, b: T) -> T { if a.total >= b.total { a } else { b } }
fn totals(xs: List[{ total: Cents, .. }]) -> Int { … }        // one record type for the whole list
fn named[T: { id: OrderId, .. } + Display](x: T) -> String { … }

due(invoice)                        // an Invoice
due(quote)                          // a Quote
larger(invoice, other_invoice)      // an Invoice, returned as one
```

- **A parameter's type is a type parameter of its own.** `fn due(x: { id: OrderId, .. })` is `fn due[X: { id: OrderId, .. }](x: X)`. Name the type parameter when the same type comes back out (`-> T`) or must be shared (`a: T, b: T`).
- **Only the named fields can be used.** `x.id` and `x.total` read the record's fields, `x.total = 5` changes a `mut` one, and `x` can be passed on to another function whose bound asks for no more. Anything else on `x` is an error. An open type (with `..`) has no values of its own: it can't be a result's, a variable's or a field's type. An anonymous record (below) can.
- **A record fits when it has every field, of a type that fits.** A field whose type is refined (`Cents`, `Int where it > 0`) must be given by a field of that same type, since the function is proven knowing the refinement. An unrefined one takes what fits it the usual way. A record's other fields don't matter, nor their order. Records stay nominal otherwise: an `Invoice` is never a `Quote`, whatever their fields.
- **Only records fit**, generic ones included (`Box[T]`). Variants, tuples, unions and other types don't, even with fields of those names. The bound is checked at every call: of a function, of a type's own method (on a value or on the type), and of a trait's method (through a trait value or a bound). A generic `impl`'s type parameters in a method's bound are the receiver's type arguments (`Store[String]`'s `put(x: { id: K, .. })` takes an `id: String`).
- **Compiled once per record type.** A call with an `Invoice` compiles `due[Invoice]`, which reads `x.total` at the offset `Invoice` keeps it at, as if `due` had been written for `Invoice`. There is no cost at run time. The cost is code size: one copy of the function per record type it is called with.
- **Verified once, for every record.** `verify` proves the function on a record holding exactly the named fields. The function can only read those fields, so what it proves holds for every record that has them. The fuzzer runs it with the first record type of the program that has the fields.

### Anonymous records

`{ id: 7, total: 10 }` is a record without a declared type. Its type is written without `..`: `{ id: Int, total: Int }`. That type can be a variable's, a result's or a field's type:

```rust
fn summary(o: Order) -> { id: Int, total: Cents } = { id: o.id, total: o.total }

type Report = { best: { id: Int, total: Cents }, count: Int }

let t: { id: Int, total: Int } = { total: 10, id: 7 }
s == t                              // equal field by field
to_string(s)                        // "{ id: 7, total: 10 }"
total_of(s)                         // fits `{ total: Cents, .. }`
```

- **How a literal is told apart.** After `{`, a field's name followed by `:` starts an anonymous record. No statement starts that way: a typed variable is `let x: T = …`. A dict literal is always `Dict { … }`.
- **One record type per set of field names.** `{ id: …, total: … }` is the generic record `type $rec:id:total[$id, $total] = { id: $id, total: $total }`, made by `desugar` once per program for each set of names it meets. Fields are laid out in order of name, so `{ total: 1, id: 2 }` has the same type, and the type prints in that order. Its fields' types are its type arguments: `{ id: Int, total: Cents }` is `$rec:id:total[Int, Cents]`. It costs what a declared generic record costs: one allocation, and fields read at fixed offsets.
- **Equality and printing** are a record's: field by field, printed as written, with no type name.
- **verify** sees an ordinary generic record. A refined field's type is a type argument, so `-> { id: Int, total: Cents }` is proven to give a non-negative `total`. An anonymous record given where a bound asks for a refined field (`total_of({ total: n, id: 1 })` against `{ total: Cents, .. }`) is proven to meet it at the call. The checker can't see the refinement of a type argument, so it leaves that to `verify`. A nominal record's field must be written with the same type instead.
- **A nominal record is not an anonymous one.** An `Order { id, total, note }` fits `{ id: Int, total: Int, .. }`, but is not a `{ id: Int, total: Int }`, even with exactly those fields. Anonymous records with the same field names are the same type, wherever they are written.
- **`derive Json`** reaches them: a type deriving `Json` that holds one derives it for the anonymous record too. Its JSON has no `"type"` key, since it has no name: `{"id": 1, "total": 2}`.

### Updating a record without naming its type

`{ ..x, total: 0 }` is `x` with `total` changed, of `x`'s type: a record, a structural parameter's type (`T` in `[T: { total: Int, .. }]`), or an anonymous record's. `x` itself is unchanged. On a `mut` variable, `x.total = 0` changes the field in place, and `x = { ..x, total: 0 }` (or `x = Order { ..x, total: 0 }`) is the same statement. `desugar` writes the assignment of an update of the same place as assignments to its fields, with each new value computed first from `x` as it was. So both forms compile to the same code: the field is set in place when `x` is held once, and copied otherwise. They also have the same proof.

A spread into a structural parameter gives it the record itself, when no field has the parameter's name: `takes(5, ...o)` with `fn takes(n: Int, x: { id: Int, .. })` is `takes(5, o)`.

## Unions, variants and patterns

A union holds a value of any of its member types. Unions are unions of sets, so they normalize: `Int | Positive` is just `Int`, while distinct types stay separate.

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

**A union either references its members or declares them.** In `type Shape = Circle | Square { side: Float } | Dot`, `Circle` refers to an existing record, `Square` is declared as a record, and `Dot` is declared as an atom (a type with a single value).

- A bare name refers to the type in scope with that name. The union declares it only when no such type exists.
- A member written with fields (`Moved(Int)`, `Square { .. }`) is always declared.
- The members can also be written one per line, each after a `|`. `type ShapeError = | Empty | TooBig { max: Int }` declares both members.
- A declared member is an ordinary type. A function can take `e: TooBig` and read `e.max`, or return `Result[Shape, TooBig]`.
- Atoms and tuple-shaped types can also be declared alone (`type Dot`, `type Moved(Int)`) to be shared by several unions.
- Constructors still need wrapping (`Err(TooBig { max: 9 })`), so `Result[Int, Int]` is unambiguous.
- A member's name must be unique in its module. A type with the same name in another module is distinguished by its module, as in `geo.Circle`.
- `edit` warns when a new member's name is close to an existing type's (`Circel` next to `Circle`).

**A declared member can have a `///` doc comment**, written above its `|`, like a record field. `std.typing` passes it to derived traits. For example, a union deriving `CommandLine` shows it as the command's help.

```rust
type Command =
  /// Show the history.
  | Log
  /// Replace definitions, read from standard input.
  | Edit { module: Option[String] }
```

**Only bare members are flattened.** With `type S = A | B`, `type S2 = S | C` is `A | B | C`. `type S3 = D(S) | E` is `D | E`, where `D` is a new type that holds an `S`.

**Patterns are sets of values.** Each arm matches what the arms above it left unmatched. A pattern can be:

- a type name: `Positive =>`, `String =>`, `List[Int] =>`, `Circle =>` (a variant, with any fields), or a trait (`Named =>`, matching values that implement it)
- a variant or record with fields: `Some(x)`, `Pending { id, .. }`
- a literal: `0`, `"x"`, `true`
- a condition on the value `it`: `where 3 <= it && it <= 5 =>`, `where len(it) > 0 =>`, `where it < 0.5 =>`
- alternatives that bind the same names: `Square | Triangle =>`, `0 | 1 | 2 =>`, `(0, x) | (x, 0) =>`
- `_`

**Narrowing.** Inside an arm, the matched variable has the arm's type, so `s` above is a `Square` with no new name needed. `if s is Square { s.side }` narrows `s` in the branch taken (and to the other variants in the `else`). To match the result of an expression, name it: `match lines(text) as ls { … }`. Matching on a field narrows the field too (`match t.status { Closed => t.status.why, … }`). Assigning to a narrowed `mut` variable ends the narrowing. A field that not every variant has can be read only where the value is narrowed to variants that have it; a call is narrowed by what its function ensures of its result (see "What a function or method says of its result's variant").

**Exhaustiveness and reachability.** The checker reports values that no arm matches (`` `where 3 <= it && it <= 5` is not covered``) and arms that earlier arms already cover (`unreachable match arm `Positive``). The matched value's refinement is taken into account: with `fn f(n: Positive)`, `match n { 1 => …, where it > 1 => … }` is exhaustive.

## Traits

A trait describes what a type can do. The implementation is chosen by the type of the value. A type can implement any number of traits, so traits form a graph rather than a hierarchy.

```rust
trait Describe { fn describe(self) -> NonEmptyString }   // a fact about the result: its type
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

- Contracts (`requires`, `ensures`) on a trait method apply to every implementation.
- `strata verify` checks laws against every implementation. The solver proves a law when it can see through the implementation's methods, which requires them to terminate and not fail. Otherwise the law is tested with generated inputs. In `examples/traits.strata`, both laws are proven for `Money` and for `List[T]`, and the solver finds that averaging (`Midpoint`) is not associative.
- A law can be a sequence of calls ending in the condition that must hold, one statement per line: `law enqueue_once(q: Self, id: Int) { mut r := q; mut r.enqueue(id); (mut r.enqueue(id)) == 0 }`. A counterexample lists the values and the steps.
- A `shared trait` (`shared trait Queue { … }`) is used by many workers at once, with its state outside the program. Each method is one atomic step. Callers keep no facts about its models from one call to the next. An implementation's method must change the outside state with a single call; a read followed by a write is a race and is rejected.
- Generic functions are fuzzed with a type that implements their bounds.

**Models: what a trait's values mean.** A trait can state what its values hold as abstract state, with `model`, and write its contracts over it. Each implementation defines the model from its own fields. `verify` proves each implementation against the contracts, and proves code that takes the trait through the model alone. From `examples/traits.strata`:

```rust
trait Bounded {
  model items: List[Int]
  model room: Positive
  invariant len(self.items) <= self.room        // holds for every value: each method may assume it and must keep it
  ensures self.room == old self.room            // holds after every `mut self` method

  fn size(self) -> Natural  ensures it == len(self.items)
  fn capacity(self) -> Positive  ensures it == self.room
  fn put(mut self, x: Int) -> Bool
    ensures it == (old len(self.items) < self.room)
    ensures self.items == if it { old self.items + [x] } else { old self.items }
}

impl Bounded for Slots {
  model items = self.held
  model room = self.limit
  ...
}

fn room_left(q: Bounded) -> Natural { q.capacity() - q.size() }   // proven by the invariant
```

- Contracts name a model as a field (`self.items`, `q.items`). Code never reads one: it calls the trait's methods.
- A contract that names a model is proven by `verify` but not checked at run time, since that would compute the model on every call.
- `examples/bank.strata` (a ledger with a law about transfers) and `examples/orders.strata` (a store of orders) are larger examples. The [reference](language-reference.md#traits) has the rules.

**Capabilities are traits.** Strata has no separate effect system. A backend such as a database, queue, console or clock is a trait. Code that uses one takes an implementation as a parameter or stores it in a field: `fn ship(mut db: Db, ...)`, `type PrintWorker = { console: Console }`. A trait used as a type means "any implementation": `db: Db` is shorthand for `fn ship[D: Db](db: D, ...)`. Name the type parameter only when it must link several types, as in `combine_all[T: Combine](first: T, rest: List[T]) -> T`. A method that changes its receiver takes `mut self` (`fn save(mut self, o: Order)`). Tests pass a `mock Db`. Iterators and tasks are separate language features (see Iterators and Tasks).

**Mocks.** A mock is declared like a type, with `mock` in place of `type`, and implemented like any type. Its implementations may omit methods:

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

- An omitted method behaves like the method of `mock Files`: it checks the method's `requires` and returns a value that satisfies its type and `ensures`.
- Only tests can use a mock. A `test`, another mock, or an `impl` for a mock may name it. Naming it anywhere else is an error, so a mock never reaches a real run.

**Capabilities provided by the runtime.** A trait that reaches the outside world is declared `capability trait`. Any type can implement it, mocks included. The real implementation is a `capability`: a set of functions that reach the outside world and hold no data. Only the runtime creates one, for `main` or for a test that asks for it by name. Its functions are called on that value, so holding the value is the permission:

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

- A capability has no constructor. `SystemClock {}`, `mock SystemClock`, JSON input and `SystemClock.seconds()` are all errors. `main`'s parameters therefore list every capability a program can use.
- `main` chooses among its capabilities in ordinary code, for example an option that picks SQLite or Postgres. It passes the rest of the program a capability trait value (`late(sys)`).
- A test can ask for capabilities the same way (`test "..." (sys: SystemClock) { ... }`). A test that doesn't uses mocks.
- `verify` never runs a capability's code, which it reports as assumed, or code that takes a capability. It proves that code from the capability's contracts, like any other code.

std defines its own capabilities: `Disk` (`std.fs`, implementing `Files`), and `Process` and `Launcher` (`std.process`, implementing `Environment` and `Commands` for running other programs). `main` asks for them by name. `Console` is the exception: the environment chooses its implementation, so `main` asks for the trait. Under `strata run`, it gets the terminal.

**Inherent methods.** `impl Item { ... }` gives a type methods of its own, outside any trait. A method that takes `self` is called on a value (`item.priced(42)`). A method without `self` is called on the type, which makes it a constructor: `Item.new("tea")`. Only records, sums and `distinct` types defined in the program can have inherent methods. They must be in the module that defines the type, in a single `impl Item` block.

```rust
impl Item {
  fn new(name: NonEmptyString) -> Item { Item { name, cents: 0 } }
  fn priced(self, cents: Natural) -> Item { Item { ..self, cents } }
}
tea := Item.new("tea").priced(42)
```

**Trait methods without `self`** are called on a type, never on a value: `Money.empty()`, `T.empty()` in a generic function, `Self.empty()` in a trait's default methods and laws. They are constructors and similar functions (`fn empty() -> Self`, `fn parse(s: String) -> Result[Self, ParseError]`), so their signature must mention `Self`.

```rust
trait Empty: Combine {
  fn empty() -> Self
  law identity(a: Self): Self.empty().combine(a) == a
}
fn combine_any[T: Empty](xs: List[T]) -> T { fold(xs, T.empty(), (acc, x) -> acc.combine(x)) }
```

## Modules: units that meet only through contracts

Each file is a module, named after its path in the package. `shop/orders.strata` is `shop.orders`, and `main.strata` is the root module. There are no `module { }` blocks.

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

- Every other module, std's included, must be imported with `use`. Only the prelude (`Option`, `Result`, `Int`, `print`, …) needs no import.
  - `use m` binds the module to its last name: `use shop.orders` gives `orders.ship(x)`.
  - `use m as a` binds it to `a`.
  - `from m use x, y as z` imports names unqualified. A type comes with its constructors, and a trait with its methods.
  - A fully qualified name (`std.json.parse`) still needs a `use` of its module. `use std` covers `std.json` too. Without the `use`, the error says which one to add.
  - A long `from` list is printed wrapped to the line width.
- Other modules see only a module's public contract: `pub` types, function signatures, `requires` and `ensures`, and trait laws. Everything else is private, and using it from another module is an error ("`secret` is private to module `a`").
- `strata interface orders` prints the public contract with its hash. Editing a private definition, or a function body without changing its signature, is reported as "implementation only", meaning nothing outside the module is affected. Changing a `pub` signature or type is reported too ("module core: its public contract changed").

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

Strata supports procedural code. `while` and `for` loop as usual, `loop { … }` repeats until `break`, and `continue` skips to the next iteration.

**Loop annotations.** Between a loop's header and its body, a loop can state:

- An invariant, which holds on every pass: `while i < n` followed by `invariant 2 * total == i * (i + 1)` on the next line. It is checked at the head of the loop at run time, and `strata verify` proves it (see [verification](verification.md#loops)). Most loops need none, because `verify` infers simple invariants.
- A termination measure: `decreases n - i`, or `decreases (left, size)` for measures compared in order (the first decreases, or it stays the same and the next one decreases). A recursive function writes its measure next to `requires` (`decreases (m, n)`). `verify` proves the measure, and finds most measures without one being written. Where it can't prove it, the measure is checked at run time (see [termination](verification-reference.md)).

`assumed invariant`, `assumed requires` and `assumed ensures` mark a clause as trusted: `verify` assumes it, and it is never checked at run time ([assumed contracts](verification.md#assumed-contracts-trusted-never-checked)).

Method chains can be split across lines:

```rust
best := scores
  .filter(s -> s > 0)
  .sorted()
  .reversed()
  .take(3)
```

**Bindings.**

- `:=` binds a new name and infers its type: `n := 0`, `(w, k) := pair()`.
- With an explicit type, the colon is already present, so the sign is `=`: `let x: Int = 5`.
- A mutable variable is `mut x := 0` or `mut x: Int = 0`.
- `=` alone assigns to an existing variable or field (`x = e`, `line.qty = …`). `+=`, `-=`, `*=`, `/=` and `%=` are shorthand for `x = x + e` and so on, and the printer uses them for such assignments.
- A function that returns `()` omits `-> ()`: `fn log(s: String) { … }`.
- A call's parentheses touch the name: `f(x)` is a call, while `f (x)` is `f` followed by `(x)`.

Variables are immutable unless declared `mut`, and every mutation is visible at the call site:

- A function that changes an argument declares the parameter `mut`. The caller passes a mutable variable and marks it `mut` at the call (`restock(mut line, 5)`). The variable is updated when the call returns. The marker is required there and is an error anywhere else.
- A method that changes its receiver takes `mut self`, and the call is marked the same way: `mut queue.claim(2)`. This is similar to Hylo's `&x.append(1)`. The marker is required for such a method and is an error on any other. It applies to the first call after the place, so `mut queue.claim(2).len()` changes `queue` through `claim` and is printed `(mut queue.claim(2)).len()`. When the marked call is an argument, it is parenthesized, `f((mut queue.pop()), x)`, to distinguish it from `f(mut queue)`, which passes `queue` to a `mut` parameter.

There are no shared references, so nothing changes behind your back. `ys := xs` makes a copy, and changing `xs` afterwards leaves `ys` unchanged. Passing a value or storing it in a field behaves the same way. There is no `copy` or `clone`. Compiled code shares the value and copies it only when one holder changes it while another still holds it (see [values and copying](#values-and-copying) for the cost). A variable's declared type holds for its whole life, and every assignment is checked against it.

**Ranges, slices and comprehensions.**

- `0..n` is the integers from `0` to `n - 1`, and `1..=n` includes `n`. A range is a `List[Int]` (`for i in 0..n { … }`, `(0..n).map(…)`).
- A range slices a list: `xs[1..3]`, `xs[..3]`, `xs[2..]`, `xs[..=i]`. Slices are clamped to the list's length, as in Python.
- `xs[i]` is one element. `i` must be in range: this is checked at run time and proven by `verify`. `get(xs, i)` returns an `Option` instead.
- An element of a `mut` list is assigned with the same syntax: `xs[i] = v`, `xs[i] += 1`, `grid[r][c] = v`, `s.items[i] = v`, `s.items[i].qty = 3`. Since values never change, `xs[i] = v` means `xs = set_at(xs, i, v)`, with the same condition on `i`. A copy held elsewhere keeps the old element. The assignment is O(1): compiled code updates the list in place when nothing else holds it.
- A comprehension builds a list from one or more others, with an optional `where` filter written as in a refinement:

```rust
[x * x for x in xs where x > 0]
[(a, b) for a in 1..=3 for b in ["x", "y"] where a != 2]
[(name, n) for (n, name) in pairs]
Dict { w: len(w) for w in words }
Set { x % 3 for x in xs }
```

**Lambdas** use an arrow: `x -> x + 1`, `(acc, x) -> acc + x`, `() -> 7`. Parameter types are optional (`(s: String) -> s + "!"`). A refined parameter type, such as `(n: Positive) -> …`, is checked on every call, as in a named function. A longer body is a block (`x -> { ... }`). `return e` leaves the enclosing function or lambda early.

A lambda with several parameters, passed where a function of one tuple is expected, destructures the tuple: `pairs.filter((k, v) -> v > 0)` and `xs.enumerate().map((i, x) -> i * x)` bind the elements with the tuple's types. This applies only when the parameters have no written types. A lambda passed where a two-argument function is expected, as in `fold`, keeps two arguments.

Any parameter, of a lambda or a function, can be an irrefutable pattern: a tuple, a record pattern, nested patterns or `_`, with or without a type. Examples: `pairs.map(((a, b)) -> a + b)`, `fold(m, 0, (acc, (k, v)) -> acc + v)`, `((a, b): (Int, Int)) -> a * b`, `fn dist((x, y): (Int, Int)) -> Int`, `(Seg { from: Pt { x, y }, to }) -> …`. The argument is bound to the pattern as `:=` would bind it. A refutable pattern, such as a variant or a literal, is rejected; take the value and `match` on it instead.

### Unused values and variables

Values never change, so a statement whose value is dropped is almost always a mistake: `xs + [1]` on its own line builds a new list and discards it. An expression statement whose value isn't `()` is an error (`the value of `xs + [1]` is not used: use it, or discard it with `_ := …``). To drop a value on purpose, write `_ := e`. This applies to:

- a call made only for its effect through a capability that also returns a value
- an `if` or `match` statement whose arms produce values (discard the arms' values, or the whole statement)
- an expression such as `x + 1`

A `mut` method call that returns `()` (`mut q.claim(2)`), a `spawn` (its task belongs to the `scope`), and `return` or `break` are allowed as statements.

Discarding a `Result` with `_ := e` is allowed, since the program may not care whether the operation failed. The error is lost, so `check`, `run`, `build` and `verify` always warn (`warning: in `main`: `_ := parse_age("")` throws away a `Result`: an error from it is ignored`). Handle it with `match`, propagate it, or leave the warning as a reminder. Discarding any other kind of value with `_ :=` produces no warning.

A variable that is never read produces a warning, since code may be half-written. This covers parameters, `x := e`, `mut x := e`, pattern bindings and lambda parameters. `check`, `run`, `build` (when it verifies) and `edit` print one line per variable, naming the function. A name starting with `_` (`_x`, `_`) marks the value as unused on purpose. The parameters of `impl` methods are exempt, since the trait or type dictates them. An unread variable whose value has no effect (a literal, a variable, a lambda, a field, or a tuple or list of these) is not compiled at all.

`check` also warns of what the whole program never uses: a private function, type or trait that nothing reaches from `main`, the tests, the `pub` and `extern` definitions and what contracts name, a field of a private record that is never read, and a variant of a private sum that is never built. `strata fix` removes the unused definitions. `[check] unused = "error"` in strata.toml makes all of these errors, unused variables included, so `check`, `run` and `build` refuse the program and `edit` rejects a change that leaves something unused. The rules are in [the tools reference](tools-reference.md).

## Values and copying

Values never change. `ys := xs + [x]` is a new list, and `xs` is unchanged. A `mut` variable changes by assignment or through a `mut` method (`mut d.insert(k, v)`), and a copy held elsewhere keeps the old value.

Compiled code makes this cheap with one rule: **a value held by one owner is updated in place, and a value with other holders is copied first.** After the copy, the new value has one owner, so the following updates are in place again. Sharing a value costs at most one copy for the updates that follow it, never one copy per update.

These updates happen in place when nothing else holds the value:

- `mut d.insert(k, v)` and `mut d.remove(k)` on a dict or set, where `d` is a variable or a field at any depth (`mut s.d.insert(k, v)`, `mut b.bids.words.remove(k)`): O(1) average.
- `xs[i] = v` (`xs = set_at(xs, i, v)`) on a list: O(1).
- `x.f = v` and `x.a.b = v`: a record or tuple field at any depth. The exception is a `distinct` record, or a record with its own refinement, where the check runs on the new record.
- `mut xs.push(e)`, `mut xs.pop()`, `xs += ys`, `xs = xs + ys`, `xs = xs + [e]` on a list: O(1) average per element added, **even when `xs` is shared**. The list's buffer is extended past its end if no other list built from it has used those slots, and copied otherwise. A copy reserves room for as many elements again, so a list grown repeatedly is copied O(log n) times.
- `s += t` and `s = s + t` on a string: in place when only `s` holds the string (and, for a field, only one owner holds each record on the path to it). Otherwise the string is copied, with room to grow.
- Containers and records with refined element types (`Dict[Int, Word]`): an update checks only what it adds (`insert`'s key and value, `push`'s element), not every element.

Every other expression builds a new value and leaves its operands unchanged. `ys := xs + [x]` copies `xs`, which keeps its own value, and so do `xs.sorted()` and `xs.reversed()`. To update in place, change the place itself: `mut xs.push(x)`, `mut xs.sort()`.

A value is held by each of the following, and changing it through one of them copies it:

- Each variable, field and container element it is stored in. After `ys := xs` the value has two holders, so the next `mut xs.insert(…)` copies it once.
- A `mut` parameter, which is the caller's variable passed by reference. `fn add(mut s: State)` changing `s.d` changes the caller's value in place, and so does a `mut self` method (`mut c.add(1)`). A field passed to a `mut` parameter, as in `bump(mut b.bids)`, is lent to the call. It is updated in place when each record on its path has one owner and no other argument of the call reads `b`.
- The container a value was read from. After `mut inner := get(outer, k) ?? Dict {}`, every `mut inner.insert(k2, v)` copies the whole inner dict. Remove it first with `mut outer.remove(k)`, change `inner`, then put it back with `mut outer.insert(k, inner)`. A list read out of a container is the exception: pushing to it is cheap (see above).
- A `for` loop, which holds what it iterates over until it ends. Changing the container inside the loop copies it once.
- A closure, which holds what it captures for as long as the closure exists.
- A function's non-`mut` parameter, which is still the caller's value. `mut e := d` followed by `mut e.insert(…)` copies `d` once.
- A sender, which still holds a value it sent to a task or channel while it keeps using it.

`strata run --copies` (also `strata test --copies` and `strata build --copies`) reports, when the program ends, each copy made to change a value. Copies are grouped by place in the code, with their count, size and reason, on standard error, largest first:

```
copies: 2 copies, 14 kB, at 1 place (the most bytes first):
  copied a Dict of up to 400 entries at app.kept@31f7e4cc/body/4/body/0/value: 2 times, 14 kB (shared: held 2 times)
```

The place is a location in the format printed by `strata find` and `strata refs`, so `strata view app.kept@31f7e4cc/body/4/body/0/value` shows it. When nothing was copied, the report is `copies: none`. Without `--copies`, the compiled code is unchanged and counts nothing.

## Errors: `Result`, `Option`, `!`, `?`, `except` and `or`

A function that can fail returns a `Result`, or an `Option` when the value may simply be absent. Each has a pair of operators: `!` and `except` for `Result`, `?` and `or` for `Option`.

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

- `e!` evaluates to the `Ok` value of a `Result`, or returns its `Err` from the function. The error must fit the function's error type: the same type, a variant of it, or a member of its union. There are no conversion functions.
- `e?` does the same for an `Option`: the `Some` value, or return `None`.
- `e except { fallback }` handles a failed `Result` in place: the `Ok` value, or else the block. `except err { ... }` binds the error. The fallback must produce the same type or leave with `return` (or `break` or `continue` in a loop: `x := opt or { continue }`). A block that always leaves produces no value, so the binding gets the `Ok` type.
- `e or { fallback }` does the same for an `Option`. There is no error to bind.
- `e !! value` and `e ?? value` are the short forms, for a fallback that is a single value on one line, and the printer always uses them for such fallbacks. They bind tighter than comparisons and looser than arithmetic: `n ?? 0 + 1` is `n ?? (0 + 1)`, and `n ?? 0 == 1` is `(n ?? 0) == 1`. They chain: `a ?? b ?? 0`.
- `!` and `?` chain too: `load(path)!.lines()`, `find(xs, p)?.name`.

## Tasks: `scope`, `spawn` and `await`

Tasks run work in parallel. A `scope` starts them, and it ends only when every task in it has finished:

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

- `spawn e` runs the whole expression `e` in a new task and returns a handle of type `Task[T]`. Like `return e`, it takes everything after it. It must be inside a `scope`.
- `await h` waits for the task and returns its result. `await (a, b)` returns a tuple of results, and `await hs` a list. `await` binds like unary `-`, so `(await hs).len()` needs the parentheses.
- There are no `async` functions. Any function can run in a task, and a call that waits (a read, a long computation) is an ordinary call. Nothing in a function's signature says whether tasks run it.

Tasks share no mutable state, so results don't depend on timing. `scope { a := spawn f(); b := spawn g(); await (a, b) }` equals `(f(), g())`, and `verify` treats it that way. A task captures what it uses as a copy at `spawn` time, like a lambda, and a value sent on a channel is the receiver's copy. The copy is real: a value the sender keeps is copied, block by block, and one it gives up on its last use moves for free (see [Values and copying](#values-and-copying)). It can't change outer variables (`spawn { n += 1 }` and `spawn f(mut xs)` are errors), and `return`, `!` and `?` can't leave it. Handle errors inside the task, or make them its result.

A fault in a task ends the program, as anywhere else. [Supervised tasks](#supervised-tasks) are the exception. A deadlock, where every task waits and none can proceed, is reported as a panic instead of hanging.

### Channels and `select`

Tasks pass values to each other through channels. `channel(n)` returns the two ends of a channel with a buffer of `n` values. `channel()` has no buffer, so each `send` waits until a receiver takes the value. The element type is inferred from use.

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

- `tx.send(v)` moves `v` into the channel, waiting while the buffer is full. `rx.recv()` returns `Some(v)`, or `None` once the channel has ended and is empty. `for v in rx` takes every value, then stops.
- Closing is explicit, once per task. Each task holding the sender calls `tx.close()` when it has finished sending. The channel ends when all of them have closed it.

`select` waits on several operations and performs only the first one that becomes possible:

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

An arm is `rx.recv()` (its `as` name is an `Option`), `tx.send(v)` (nothing to name) or `await h` (the task's result). An arm that is not chosen has no effect: its value is not sent, and the task it awaits keeps running. A `recv` on an ended channel is always ready and yields `None`, so a loop over several channels can tell when each one ends. When several arms are ready, which one runs is unspecified.

### `race`

`race` has the same arms as `select`, but the left side of each arm is any expression, run as its own task. The first to finish runs its arm with its result bound to the `as` name, and the others are cancelled:

```rust
fn first_answer(mirrors: Receiver[Page], local: Cache, key: String) -> Page {
  race {
    mirrors.recv() as p => p ?? not_found(key),
    lookup(local, key) as hit => hit,
  }
}
```

A cancelled task stops at its next wait (a `recv`, `send`, `await`, `sleep`, …), however deep in the call stack, and unwinds as `return` would.

### Supervised tasks

A panic ends the program, wherever it happens. A task started with `supervised spawn` is the exception. Its panic ends only that task and the tasks it started, and awaiting it returns a `Result[T, Panic]` instead of a `T`. The failure is handled where the task is awaited, like any `Result`:

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

`Panic` is `{ kind: String, message: String, stack: String }`. `kind` is `Runtime`, `Requires`, `Refinement`, and so on. `stack` is the call stack from `main` through each `spawn`.

### Time: `Clock`

Time comes from outside the program, so it is a capability (`std.time`). `main` or a test asks for the `SystemClock` and passes it on as a `Clock`.

- `clock.now()` returns an `Instant`. Instants are monotonic, so only the difference between two is meaningful: `elapsed(a, b)`.
- `clock.sleep(d)` waits for a `Duration`, made with `nanos`, `micros`, `ms` or `seconds`. `.value` is in nanoseconds.

Sleeping is a wait, like `recv`, so a cancelled task stops sleeping immediately. A `race` therefore makes a timeout:

```rust
fn fetch_or_give_up(clock: SystemClock, net: Net, url: String) -> Option[Page] {
  race {
    fetch(net, url) as p => Some(p),
    clock.sleep(seconds(2)) => None,
  }
}
```

A test of timeouts asks for a `VirtualClock`, a capability like `SystemClock`. Each test, and the program for `main`, has its own virtual time. It starts at 0 and is shared by the test's tasks. It advances by itself: once every task is waiting and at least one waits on this clock (a `sleep` or an `after` timer), the time jumps to the nearest deadline. A timeout then fires immediately and at the exact time:

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

Every program has these types without a `use`:

| Type | Notes |
|---|---|
| `Int`, `Float`, `Bool`, `String` | `Int` is arbitrary-precision, like Python's: a machine integer while it fits, a big integer when it doesn't. `Float` is never NaN or infinite. Ints and Floats never mix implicitly (`float(n)`, `round(x)`). Integer `/` and `%` are Euclidean, so the remainder is never negative: `-7 / 2 == -4`, `-7 % 3 == 2`, `7 % -3 == 1`. For C's truncating division, divide the magnitudes and restore the sign (`-(7 / 2) == -3`). `parse_int` accepts an optional `+` or `-` sign (`parse_int("+5") == Some(5)`), so check the first character when only digits are allowed. Strings are UTF-8, ordered (`<`, `<=`, `>`, `>=`, `compare`, `sort`) by code point, which matches UTF-8 byte order |
| `Real` | exact rational numbers, the type of decimal literals: `0.1` is one tenth, so `0.1 + 0.2 == 0.3`. `+ - * / %` and comparisons are exact; `/` and `%` by zero is a check, as for `Int` ("division by zero", left out where `verify` proves the divisor nonzero). `1.0 / 3.0` prints `1/3`, a terminating decimal prints as one (`2.5`, `3.0`). `floor`, `ceil`, `round` give `Int`s; `abs`, `min`, `max`, `sum` work as on `Float`; `x.to_float()` is the nearest `Float` (`None` past the largest), `Real.from(n)` an `Int` as a `Real`, `Real.from_float(f)` a `Float`'s shortest digits, `Real.parse("1/3")` reads one (`-2.5`, `6.02e23` too). `match` takes decimal and integer literals as patterns (`0.5 =>`, `2 =>`). `sqrt`, `sin`, `exp` and the like are `Float`'s only. A `Real` is made only by literals, those functions and arithmetic, never from its parts (`r.num` and `r.den` can be read). JSON and TOML numbers and the ORM's `Real` columns are exact. Cost: two words; a literal is a constant, a result stays on the stack or in its variable's block; each operation is a few machine-word steps and a GCD while both parts fit in 62 bits (adding `0.1` takes about 15 ns, a `Float` addition about 1.5 ns). Values grow with the digits they need (100 compoundings of `x * 1.07` give a 200-digit denominator), so numeric code that can round says `Float` |
| `I8` … `I64`, `U8` … `U64` | fixed-width integers. Their arithmetic wraps around, and they never mix with `Int` (`x.to_int()`, `n.to_u32()`) |
| `()` | the unit type, for "no meaningful value". It plays the role of Python's `None` as a type |
| `Option[T]` | `None` or `Some(x)`: a value that may be absent |
| `Result[T, E]` | `Ok(x)` or `Err(e)`: the outcome of an operation that may fail |
| `List[T]` | an ordered sequence, `[1, 2, 3]` |
| `(A, B, …)` | tuples, `(word, 3)`, destructured with `(w, n) := …` and accessed with `t.0` |
| `Dict[K, V]` | a hash map, `Dict { "a": 1, "b": 2 }`, with `get`, `insert`, `remove`, `keys`, `values`, `items` |
| `Set[T]` | a hash set, `Set { 1, 2 }`, with `insert`, `union`, `intersection`, `difference` |

**Number literals.** A decimal literal is a `Float` where a `Float` is wanted: an annotation (`let x: Float = 0.5`), a parameter or field, a function's result, the other operand of an operator (`x * 0.5`), the other branch of an `if`, another argument of a builtin (`max(x, 0.5)`), or a later use of the variable it is in (`mut t := 0.0` then `t = t + x.area()`). Anywhere else it is a `Real`. An integer literal is an `Int`, a `Real` where a `Real` is wanted (`x * 2`, `let r: Real = 3`), or a sized integer where one is wanted. `verify` proves `Real` code exactly, as SMT's reals; `Float` code is not modelled.

All values are immutable: `insert` returns a new dict. `+` adds numbers and concatenates strings and lists. String operations always name their unit: `s.bytes()`, `s.code_points()` or `s.graphemes()`.

**f-strings** interpolate any expression, with Python's format specs: `f"{name} has {len(xs)} items"`, `{price:.2}`, `{n:>5}`.

Every value can be printed. `debug(x)` writes it as code. `to_string(x)`, which `print` and f-strings use, returns a string's text unchanged and anything else's `debug` form. A type overrides either with an `impl` of `Debug` or `Display`. `strata builtins` lists every builtin function with its signature.

**Iterators.** A `for` loop runs over lists, sets and dicts, and over any value whose type implements the prelude's `Iterator`. `next` returns the elements one at a time, then `None`.

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

Every iterator has lazy `map`, `filter` and `enumerate` (which pairs each element with its index, `(i, x)`). An iterator that always ends implements `FiniteIterator` too, and `verify` then proves that a `for` loop over it terminates. `xs.iter()` iterates over a list, set or dict, and a string's units are iterators.

A finite iterator is converted to a list, as if `to_list(...)` were written, wherever a `List` is expected or the value is displayed. That includes passing it as a `List` argument (`count(s.graphemes())`), comparing it with a list (`s.graphemes() == ["a", "b"]`), printing it, interpolating it, and passing it to `to_string` (`print(s.graphemes()[1..3])` prints `["b", "c"]`). Each conversion walks the iterator once, in O(n). An iterator that may not end can't be converted; the error suggests taking a prefix first (`.take(n)`).

## Programs and the standard library

A program's entry point is `main`. It declares what it needs by type, and `strata run` provides it: the command-line options and the capabilities it uses.

```rust
from std.cli use CommandLine

/// Greet people by name.
derive CommandLine for
type Options = {
  /// Shout the greeting.
  loud: Bool = false,
  /// Say it this many times.
  times: Int where 1 <= it && it <= 10 = 1,
  /// Who to greet (the world if nobody).
  names: List[String] = [],
}

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

**`print(x)`** is always available without a `use`. It writes `x`, formatted by `to_string`, followed by a newline. Under `strata run` the output goes to the terminal. Under `strata test` it goes to the test's own buffer, shown only if the test fails. Under `verify` it is discarded. Code whose output a test should check, or that a caller should control, takes a `Console` instead.

**Command-line options** are a record that derives `CommandLine` (`std.cli`). Each field is parsed from the arguments according to its type: a `Bool` is a flag, a `List[T]` takes the remaining arguments, and a field with a default is an option. Every refinement is checked, and `--help` is generated from the `///` doc comments. A union that derives `CommandLine` defines subcommands (`Edit` becomes `edit`), each with its own options.

`main` can also take a `Console` and capabilities (`Disk`, `Process`, `Net`, or the program's own), in any order. It passes them on where a trait (`Files`, `Environment`) is expected, so tests can substitute their own implementations. `main` returns `()`, an `ExitCode`, or a `Result` of either. An `Err` is written to standard error and the exit code is 1.

The **standard library** is the `std/` package. It is written in Strata and built into `strata`, so any program can `use std.…` without declaring a dependency. It covers:

- the console, files, time and processes (`std.io`, `std.fs`, `std.time`, `std.process`)
- SQLite databases (`std.sqlite`), and records stored as tables (`std.orm`, `derive Table`)
- command lines (`std.cli`)
- sorted dictionaries and sets and a priority queue (`std.collections`)
- JSON and TOML (`std.json`, `std.toml`)
- TCP connections, HTTP clients and servers, HTTPS clients (`std.net`, `std.http`), and HTML pages written as markup (`std.web`)
- an LLM agent loop with tools (`std.agent`)
- regular expressions and Unicode (`std.regex`, `std.unicode`)

Each module has a [reference page](std/README.md), generated from its doc comments by `strata docgen`. `strata doc std.json` shows one in the terminal.

**JSON.** A type that derives `Json` can be read from and written to JSON, with every refinement checked:

```rust
from std.json use Json, parse, stringify

derive Json for
type Signup = { email: String where contains(it, "@"), age: Int where it >= 13, nickname: Option[String] }

signup := Signup.from_json(parse(body)!)!
text := stringify(signup.to_json()!)  // {"type":"Signup","email":…,"age":…,"nickname":null}
```

**HTML pages.** Markup is written in Strata code, as in JSX, and an element is an `Html[M]` value (`std.web`):

```rust
from std.web use Html, document

fn row(l: Language) -> Html[()] {
  <tr>
    <td>{l.name}</td>
    <td class="num">{l.year}</td>
  </tr>
}

fn table(ls: List[Language]) -> Html[()] {
  <table><tbody>{ls.map(l -> row(l))}</tbody></table>
}

page := document("Languages", [], [<h1>Languages</h1>, table(ls)])
```

- `{e}` puts a value in: a `String` or `Int` as text, an `Html` as an element, a list of them in its place. Text and values are escaped when rendered, attributes too, so a name like `<script>` shows as written. Only `raw_html(s)` writes HTML as it is.
- An attribute is `name="text"`, `name={e}` (a `String`, an `Int`, or a `Bool` for a flag that is there when `true`), or a bare `name` (a flag). An event is `on_click={message}`. `onclick="…"` is refused: a page can't hold inline scripts.
- A component is a function returning `Html`, called in braces: `{row(l)}`. A tag is lowercase (`<div>`, `<my-card>`).
- An element without children is written `<br />`, and `<>…</>` is a list of elements.
- The module needs an import from `std.web` (such as `from std.web use Html`). The parser reads markup as calls to `std.web` (`std.web.element("td", [std.web.attr("class", "num")], std.web.children([l.year]))`), which is what is stored, checked and compiled; `view` shows those calls as markup again. A literal tag lets `verify` prove that void elements (`br`, `input`, …) get no children. Rendering is one pass over the tree (`render`, `document`).

**Foreign functions.** C functions are declared with `extern c fn`: a signature with contracts and no body (see the [reference](language-reference.md#foreign-functions-extern-c)). Python functions are declared with `extern python "module" fn`. Their values cross the boundary as JSON (`Json`), and every call returns a `Result[T, PythonError]` (see the [reference](language-reference.md#foreign-functions-extern-python)).
