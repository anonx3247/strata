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
  fn save(mut self, o: Order) -> ()        // `mut self`: saving changes the store
}

fn ship(mut db: Db, id: OrderId, tracking: NonEmptyString) -> Result[Order, ShipError]
  ensures match it { Ok(o) => o is Shipped, Err(_) => true }
{
  let order = db.load(id) or { return Err(NotFound) }
  match order {
    Pending { id, lines } => {
      let o = Shipped { id, lines, tracking }
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
  let mut db = mock Db
  let r = ship(mut db, "ord_zzz999", "TRK9")
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
let id = UserId("u_42")      // explicit construction, checked
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
let r = Retry { attempts: 5 }      // backoff: [100, 200], label: "retry"
let d = Retry {}
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

**Shared method names.** Two traits may declare methods with the same name, and each implementation keeps its own body. A call picks by what is known about the value it is called on:

- known only through a trait (`d: Describe`, `T: Describe`): that trait's implementation;
- a known type: its own method if it has one (`impl Item { fn describe(self) … }`), else the one trait of the type that declares the name. This holds even when a function of another module has the same name: inside your own `fn handle`, `app.handle(req)` is `Router`'s method (`handle(app, req)` would be the function);
- otherwise it is a compile error: a type implementing two traits that both have `describe`, with no `describe` of its own; a value known only as `Describe | Log` or `T: Describe + Log`. In the last two cases a type's own `describe` doesn't help: it says what `describe` means on an `Item`, not which trait's meaning the call wants.

This holds whichever modules the traits come from: with `Console` (`std.io`) and `Files` (`std.fs`) both imported, `mut console.write(s)` and `mut files.write(path, s)` each mean their own trait's `write`. A method named like a builtin (`Files.remove`) doesn't hide the builtin: on a value whose type has no trait declaring the name (a dict, a list, a string…), `d.remove(k)` and `remove(d, k)` are the builtin's.

`strata` decides this when checking and writes the decision into the program it runs, so the same call always runs the same implementation. A name that depends on the receiver can't be passed as a bare function value (`map(describe)`): pass a lambda (`x -> x.describe()`).

**Trait methods without `self`** are called on a type, never on a value: `Money.empty()`, `T.empty()` in a generic function, `Self.empty()` in a trait's default methods and laws. They are constructors and the like (`fn empty() -> Self`, `fn parse(s: String) -> Result[Self, ParseError]`), so the signature must mention `Self`: one that doesn't couldn't pick an implementation, and should be a plain function.

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
- a method that changes its receiver takes `mut self`, and the call is marked the same way: `mut queue.claim(2)` (as Hylo writes `&x.append(1)`). The mark is required for such a method, and an error on any other. It marks the first call on the place after it: `mut queue.claim(2).len()` changes `queue` through `claim`, and is written `(mut queue.claim(2)).len()`. As an argument, the marked call is put in parentheses, `f((mut queue.pop()), x)`, to tell it from `f(mut queue)`, which passes `queue` to a `mut` parameter.

Only a variable declared `let mut`, a `mut` parameter, or a field of one can be changed: passed `mut`, or marked as a receiver. A call's result is a value, not a place, and the changes would have nowhere to go: `mut first(xs).tick()` is an error. Name it first (`let mut x = first(xs)`).

There are no shared references, so nothing changes behind your back. A declared type holds for the variable's whole life: every assignment is checked against it (`line.qty = 120` fails if `qty` is a `Quantity`; `x = -1` fails if `x` was declared `let mut x: Positive`), at compile time for literals and at runtime otherwise, and so is the value a `mut` argument writes back. Passing a `Positive` variable to a `mut k: Int` parameter is a compile error, since the function could store any `Int` in it. Closures cannot change outer variables.

**Ranges, slices and comprehensions.** `0..n` is the integers from `0` to `n - 1`, and `1..=n` includes `n`; a range is a `List[Int]` (`for i in 0..n { … }`, `(0..n).map(…)`). A list or string is sliced with a range: `xs[1..3]`, `s[..3]`, `xs[2..]`, `xs[..=i]`, clamped to its length as in Python. There are no negative indices: the end of `xs` is `len(xs)`, as in `xs[..len(xs) - 1]`. One element is `xs[i]` (`s[i]` is a one-character string): `i` must be in range, which is checked when the code runs and proven by `verify` (`xs[0]` on a `NonEmpty[Int]` is proven; `xs[len(xs) - 1]` on any list is not). `get(xs, i)` gives an `Option` instead. A comprehension builds a list from one or more others, with a `where` to keep only some elements, as in a refinement:

```rust
[x * x for x in xs where x > 0]
[(a, b) for a in 1..=3 for b in ["x", "y"] where a != 2]
[(name, n) for (n, name) in pairs]
Dict { w: len(w) for w in words }
Set { x % 3 for x in xs }
```

They are written in the tree as such, so they render back as written. When the program is resolved they become the builtins (`range`, `slice`, `map`, `filter`, `fold`), so the checker, the interpreter and the prover treat them exactly as those calls.

Lambdas are written with an arrow: `x -> x + 1`, `(acc, x) -> acc + x`, `() -> 7`, with optional parameter types (`(s: String) -> len(s)`; a refined one, such as `(n: Positive) -> …`, is checked on every call, like a named function's) and a block for a longer body (`x -> { ... }`). `return e` leaves the enclosing function or lambda early.

## Errors: `Result`, `Option`, `!`, `?`, `except` and `or`

A function that can fail returns a `Result` (or an `Option` for "maybe absent"). Each has its own pair of operators: `!` and `except` for a `Result`, `?` and `or` for an `Option`.

```rust
fn transfer(from: Account, to: Account, amount: Cents) -> Result[(Account, Account), WithdrawError] {
  let from = withdraw(from, amount)!        // an Err is returned from `transfer` as is
  Ok((from, Account { ..to, balance: to.balance + amount }))
}
fn status_of(q: MemQueue, id: TaskId) -> Option[Status] {
  Some(q.entries.find(e -> e.task.id == id)?.status)  // None is returned from `status_of`
}
let port = parse_int(text) ?? 8080                          // an Option: its value, or a fallback
let user = find_user(id) or { return not_found() }          // ... or a block, which may return
let n = parse(s) !! 0                                       // a Result: its value, or a fallback
let n = parse(s) except e { return describe_error(e) }      // ... or a block, with the error by name
```

- `e!` takes the `Ok` value of a `Result`, or returns its `Err` from the function. The error must fit the function's error type: the same type, or a variant of it, or a member of a union (there is no conversion function).
- `e?` does the same for an `Option`: the `Some` value, or return `None`.
- `e except { fallback }` handles a failed `Result` here: the `Ok` value, or else the block. `except err { ... }` names the error. The fallback must produce the same type, or leave with `return`.
- `e or { fallback }` does the same for an `Option`: the `Some` value, or else the block (`None` has nothing to name).
- `e !! value` and `e ?? value` are the short forms, for a fallback that is a single value on one line; such a fallback is always written that way. They bind tighter than comparisons and looser than arithmetic (`n ?? 0 + 1` is `n ?? (0 + 1)`, `n ?? 0 == 1` is `(n ?? 0) == 1`), and chain: `a ?? b ?? 0`.
- Each works on its own type only: `except` / `!!` on an `Option`, or `or` / `??` on a `Result`, is an error. So a `Result[Option[T], E]` reads one way: `(load(k) !! None) ?? default` handles the error, then the missing value.
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
| `Dict[K, V]` | `Dict { "a": 1, "b": 2 }`, `Dict[String, Int] {}`, `dict(pairs)` from a list of pairs, `get`, `insert`, `remove`, `keys`, `values`, `items`; JSON object when keys are strings |
| `Set[T]` | `Set { 1, 2 }`, `Set[Int] {}`, `set(xs)` from a list, `insert`, `union`, `intersection`, `difference`, `to_list` |

`+` adds numbers and joins strings and lists (`"a" + "b"`, `[1] + [2]`). **Strings** are UTF-8 and indexed by character: `len`, `chars`, `s[i]`, `get(s, i)`, slices `s[start..end]` (clamped, as in Python; also on lists), `index_of`, `contains`, `starts_with`, `ends_with`, `split`, `join`, `lines`, `trim`, `replace`, `repeat`, `reverse`, `to_upper`, `to_lower`, `is_digit` / `is_alpha` / `is_space`, `ord` / `chr`, `parse_int`, `matches` (regex). `to_json(x)` gives any value as compact JSON, as `strata run` writes it. **f-strings** interpolate any expression, shown as by `to_string`: `f"{name} has {len(xs)} items"` (`{{` and `}}` are literal braces). A **format spec** follows a `:`, as in Python: `[[fill]align][+][0][width][.precision]`. `{price:.2}` gives two decimal places (or keeps at most 2 characters of a `String`), `{name:<12}` / `{n:>5}` / `{title:*^20}` pad to a width (numbers align right by default, everything else left), `{n:05}` pads with zeros after the sign, `{delta:+}` always shows the sign. A spec that doesn't suit the type is a check error.

All values are immutable (`insert` returns a new dict). Dicts and sets are kept in canonical order, so equal collections are equal values and serialise identically. `len`, `contains`, `get`, `insert`, `remove`, `sum`, `min`, `max` and `abs` work across the types they make sense for. `strata builtins` lists every builtin with its signature. See `examples/builtins.strata`.

**Not yet:** lazy iterators, generators and async, which will be designed as language features of their own.

## Programs and the standard library

A program's entry point is `main`. It asks for what it needs by type, and `strata run` passes it in: its command-line options, and the capabilities it uses.

```rust
use std.cli.CommandLine

/// Greet people by name.
type Options = {
  /// Shout the greeting.
  loud: Bool = false,
  /// Say it this many times.
  times: Int where 1 <= it && it <= 10 = 1,
  /// Who to greet (the world if nobody).
  names: List[String] = [],
}

impl CommandLine for Options {}

fn main(opts: Options) -> () {
  let who = if len(opts.names) == 0 { "world" } else { join(opts.names, " and ") }
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

**Command-line options** are a record that implements `CommandLine` (`std.cli`); the trait has no methods. Each field is read from the arguments by its type: a `Bool` is a flag (`--loud`), an `Option[T]` an option with a value (`--times 3` or `--times=3`), a `List[T]` the remaining positional arguments, and any other field a required positional argument, in field order, or an option (`--times 3`) when it has a default. A field left out takes its default. `T` is an `Int`, `Float`, `String`, a refinement of one, or a choice (a type whose variants carry nothing: `type Count = | Lines | Words` is `lines` or `words`). Names take dashes (`dry_run` is `--dry-run`), `--` ends the options, every refinement is checked (`--times: 0 does not satisfy …`), and a mistake exits with code 2. `--help` is built from the record's and fields' `///` doc comments. `strata check` rejects a `CommandLine` record a command line can't fill.

`main` can also ask for `Console` (`std.io`), `Files` (`std.fs`) and `Environment` (`std.process`), in any order. It returns `()` (exit code 0), an `ExitCode` (`Int where 0 <= it && it <= 255`), or a `Result` of either: an `Err` is written to standard error and the exit code is 1. A runtime fault exits with 101. `strata run <fn> [JSON-ARGS]` still calls a single function.

The **standard library** is the `std/` package, written in Strata and built into `strata`, so any program can `use std.…` without declaring a dependency:

| Module | What it has |
|---|---|
| `std.io` | `Console` (`print`, `write`, `eprint`, `read_line`); `Recorder` / `recorder(input)` / `printed(r)` for tests |
| `std.fs` | `Files` (`read`, `write`, `append`, `exists`, `list`, `make_dir`, `remove`), `FsError`; `MemFiles` / `mem_files(files)` for tests |
| `std.process` | `Environment` (`args`, `var`), `ExitCode`; `FakeEnvironment` / `fake_environment(args, vars)` for tests |
| `std.cli` | `CommandLine`: a record of options read from the arguments |
| `std.http` | `Request`, `Response`, `Method`; `ok_html`, `ok_text`, `ok_json`, `not_found`, `redirect`, …; `Router` (`app.get("/users/:id", handler)`, `req.param("id")`), for `strata serve` |
| `std.web` | `Html[M]` and `Attr[M]`: HTML as values, one function per element (`div`, `p`, `table`, `button`, …) and attribute (`class`, `href`, `on_click(msg)`, …); `render`, `document(title, head, body)` |

**Web servers** (`std.http`) define `fn handle(req: Request) -> Response`, which `strata serve` calls for each request. Routes are written as with Hono or Express, on a `Router`:

```rust
fn app() -> Router {
  let mut app = Router.new()
  app.get("/", req -> redirect("/users"))
  app.get("/users/:id", show_user)
  app
}

fn show_user(req: Request) -> Response {
  let id = req.param("id") or { return not_found() }
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

Events carry a message (`on_click(Reset)`), or make one from what the user entered: `on_input(s -> Typed { text: s })` for a field's value as it is typed, `on_change(make)` for a select or a checkbox (`"true"` / `"false"`), and `on_submit_form(fields -> Submitted { fields })` for a form's fields by name (a `Dict[String, String]`). Checking what was entered is ordinary code in `update`, so a refused form can keep its error in the model and show it next to the field (see `examples/todos`).

**HTML** (`std.web`) is built as values and rendered to text; see `examples/page`. Text is always escaped. Tag and attribute names are checked where they are written: `element` and `attr` take a `TagName` and an `AttrName`, and an attribute can't be an inline script (`attr("onclick", …)` is rejected when checking). `Html` and `Attr` are records around private types, so only those functions make them; the types themselves have nothing left to check, and the verifier passes over views. Events are attributes too: `on_click(msg)` carries a value of the page's message type `M`, written as JSON in a `data-on-click` attribute for the runtime to send back (the server that does so isn't built yet). A page without events is an `Html[()]`. The text inside a `<style>` element (`style_sheet(css)`) isn't escaped, but can't close the element.

Each capability is a trait, so code that prints takes a `Console`, and its signature says so. The only real implementations are private types of `std` that the runtime makes for `main`; nothing else can create them. Tests pass the in-memory stand-ins (see `examples/wc`), or `mock Console`. Only `std` can call the native `host_*` operations underneath, and only `strata run` lets them reach the system: `test`, `verify` and fuzzing never touch the terminal or the disk.
