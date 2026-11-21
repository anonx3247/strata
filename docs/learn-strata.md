# Learn Strata in Y minutes

Strata is an experimental language for working with agents: constraints live in the types, contracts are proven by an SMT solver (or fuzzed when they can't be), unfinished code is a typed hole, and side effects are capabilities passed as parameters. Its syntax is close to Rust's, with Python's integers and f-strings.

This page is meant to be read in one go, by a person or an agent, before writing Strata. The code below is a single valid program, exactly as `strata render` prints it: a test stores it, checks it and runs its tests, so it can't drift from the language. To try it, save it as text in `learn.st`, store it with `strata --file learn.strata edit < learn.st`, then run `strata --file learn.strata verify`.

For more depth: [the language](language.md), [verification](verification.md), [tools](tools.md).

```rust
// A comment belongs to what follows it; /// doc comments describe the next
// definition, field or variant (the viewer and --help show them).

// ─── 1. Values and expressions ─────────────────────────────────────────────
fn basics() -> Int {
  // `x := e` names a value, its type inferred; `let x: T = e` gives the type.
  // Both are immutable (`mut x := e` and `mut x: T = e` are not).
  n := 42
  let x: Float = 3.5
  ok := true && !false || n == 42
  s := "hello"

  // Int never overflows (it grows into a big integer). Float is never NaN.
  // No implicit Int/Float mixing: convert with float(n), round(x), floor, ceil.
  mixed := float(n) * x
  back := round(mixed)

  // f-strings interpolate any expression; specs after `:` as in Python.
  msg := f"{s}, {n} items, {x:.2}, {n:>5}, {{literal braces}}"

  // `+` adds numbers and joins strings and lists.
  greeting := s + " world"
  both := [1, 2] + [3]

  // Everything is an expression: `if` and blocks have values.
  sign := if n > 0 {
    "+"
  } else if n < 0 {
    "-"
  } else {
    "0"
  }
  y := {
    a := 1
    a + 1
  }
  back + y + len(greeting) + len(both) + len(msg) + len(sign)
}

// ─── 2. Functions ──────────────────────────────────────────────────────────
// Parameters and the result are always typed. The last expression is the result.
fn add(a: Int, b: Int) -> Int {
  a + b
}

// `()` is the unit type: "no meaningful value".
fn shout(s: String) {
  print(to_upper(s))
}

// Lambdas: `x -> x + 1`, `(a, b) -> a + b`, `() -> 7`, `x -> { ... }`.
// Method-call syntax works on any function: xs.map(f) is map(xs, f).
fn evens_squared(xs: List[Int]) -> List[Int] {
  xs.filter(x -> x % 2 == 0).map(x -> x * x)
}

// A named function can be passed as a value (an overloaded builtin such as
// `len` needs a lambda: `w -> len(w)`).
fn line_totals(lines: List[Line]) -> List[Cents] {
  lines.map(line_total)
}

// ─── 3. Refinement types: a type plus a condition on `it` ──────────────────
type Quantity = Int
  where 1 <= it && it <= 99

type Cents = Int
  where it >= 0

type Sku = String
  where matches(it, "[A-Z]{3}-[0-9]{1,4}")

// Refinements are structural: any Int may be passed as a Quantity; the check
// happens at the boundary (compile time for literals, run time otherwise, and
// proven or refuted by `strata verify`). Inline refinements need no name:
fn pick(level: Int where 5 <= it && it <= 13) -> Positive {
  level - 4
}

// A result's refinement can mention parameters.
fn halve(n: Natural) -> Int where it >= 0 && it * 2 <= n {
  n / 2
}

// The prelude has: Natural, Positive, Negative, NonZero, Probability,
// Percentage, NonEmpty[T], NonEmptyString, and I8..I64 / U8..U64.
// `type Name = String` alone is an error: add `where`, or use `distinct`.

// A distinct type is nominal: it never mixes with its base or other types.
distinct type UserId = String
  where matches(it, "u_[0-9]+")

fn user_number(id: UserId) -> String {
  // build one with UserId("u_42"); `.value` is the underlying String
  id.value[2..]
}

// ─── 4. Contracts: requires / ensures (`it` is the result) ─────────────────
fn divide(a: Int, b: Int) -> Int
  requires b != 0
  ensures b > 0 && a >= 0 ==> it >= 0 {
  a / b
}

// ─── 5. Records, variants and unions ───────────────────────────────────────
type Line = { sku: Sku, qty: Quantity, unit_price: Cents }

// A sum type. Each variant is itself a type (`Shipped` can be a parameter type),
// so variant names are unique across the whole project.
type Order =
  | Pending { id: NonEmptyString, lines: NonEmpty[Line] }
  | Shipped { id: NonEmptyString, lines: NonEmpty[Line], tracking: NonEmptyString }
  | Cancelled { id: NonEmptyString, reason: NonEmptyString }

// Variants without fields make an enum; tuple-style variants exist too.
type Size =
  | Small
  | Large

// A union of existing types. It normalizes like a set (`Int | Positive` is Int).
type Square = { side: Positive }

type Circle = { radius: Positive }

type Shape = Square | Circle

// Generic types take parameters in brackets.
type Pair[A, B] = { first: A, second: B }

// A field can have a default, after its type; a constructor may leave it out.
type Retry = {
  attempts: Int where 1 <= it && it <= 10 = 3,
  label: String = "retry",
}

fn records() -> Line {
  line := Line { sku: "TEA-7", qty: 2, unit_price: 450 }
  qty := line.qty
  // Functional update: copy with some fields changed.
  more := Line { ..line, qty: qty + 1 }
  // Shorthand: `Line { sku, qty, unit_price }` when variables have those names.
  p := Pair { first: 1, second: "one" }
  t := (more.sku, p.first)
  // Tuples: destructure with :=, or read t.0, t.1.
  (code, count) := t
  r := Retry { label: "fetch" }
  Line { ..more, sku: code, qty: min(count + r.attempts, 99) }
}

// ─── 6. Pattern matching: exhaustive, and arms narrow ──────────────────────
fn line_total(l: Line) -> Cents {
  l.qty * l.unit_price
}

fn status(o: Order) -> String {
  match o {
    Pending { id, .. } => f"{id} is pending",
    // `o` is narrowed: inside this arm it is a Shipped, so o.tracking works.
    Shipped => f"shipped: {o.tracking}",
    Cancelled { reason, .. } => f"cancelled: {reason}",
  }
}

fn area(s: Shape) -> Int {
  match s {
    Square => s.side * s.side,
    Circle => 3 * s.radius * s.radius,
  }
}

fn kind_of(v: Int | String) -> String {
  match v {
    Int => f"number {v}",
    String => f"text of {len(v)}",
  }
}

// Patterns: a type name, a variant (with fields), a literal, a condition on
// `it`, alternatives with `|`, list and tuple shapes, and `_`.
fn classify(n: Int) -> String {
  match n {
    0 => "zero",
    1 | 2 | 3 => "small",
    where it < 0 => "negative",
    where 4 <= it && it <= 99 => "medium",
    _ => "large",
  }
}

// Lists match by shape; `..rest` binds the remainder.
fn length(xs: List[Int]) -> Natural {
  match xs {
    [] => 0,
    [_, ..rest] => 1 + length(rest),
  }
}

// Match on a computed value by naming it with `as`.
fn first_word(text: String) -> String {
  match split(text, " ") as ws {
    [w, ..] => w,
    [] => "",
  }
}

// `x is Pattern` tests without matching.
fn is_open(o: Order) -> Bool {
  o is Pending
}

// ─── 7. Errors: Option, Result, `!` `?`, `except` `or`, `!!` `??` ───────────
type ParseError =
  | Blank
  | NotANumber { text: String }

fn parse_qty(s: String) -> Result[Quantity, ParseError] {
  if len(trim(s)) == 0 {
    return Err(Blank)
  }
  match parse_int(trim(s)) {
    Some(n) => if 1 <= n && n <= 99 {
      Ok(n)
    } else {
      Err(NotANumber { text: s })
    },
    None => Err(NotANumber { text: s }),
  }
}

// e!  : the Ok value, or return the Err from this function.
// e?  : the Some value, or return None from this function.
fn double_qty(s: String) -> Result[Int, ParseError] {
  q := parse_qty(s)!
  Ok(q * 2)
}

fn first_upper(words: List[String]) -> Option[String] {
  Some(to_upper(words.find(w -> len(w) > 0)?))
}

// Handle it here instead. A Result: `e !! fallback`, or `e except err { … }`.
// An Option: `e ?? fallback`, or `e or { … }`. A block may `return`.
// Each pair works on its own type only.
fn qty_or_one(s: String) -> Int {
  parse_qty(s) !! 1
}

fn port(text: String) -> Int {
  parse_int(text) ?? 8080
}

fn first_long(words: List[String]) -> String {
  w := words.find(w -> len(w) > 3) or { return "none" }
  to_upper(w)
}

fn explain(s: String) -> String {
  q := parse_qty(s) except e {
    return match e {
      Blank => "blank",
      NotANumber { text } => f"not a number: {text}",
    }
  }
  f"quantity {q}"
}

// ─── 8. Mutation: immutable by default, always visible ─────────────────────
fn total(lines: List[Line]) -> Natural {
  mut sum: Natural = 0
  for line in lines {
    sum += line_total(line)
  }
  sum
}

// A `mut` parameter changes the caller's variable; the call says `mut` too.
fn restock(mut line: Line, extra: Natural) {
  line.qty = min(line.qty + extra, 99)
}

fn loops() -> Int {
  mut line := Line { sku: "CUP-1", qty: 1, unit_price: 1200 }
  restock(mut line, 5)
  mut i := 0
  while i < 10 {
    i += 1
  }
  loop {
    i -= 1
    if i % 2 == 1 {
      continue
    }
    if i < 3 {
      break
    }
  }
  // `0..3` is 0, 1, 2 and `1..=3` is 1, 2, 3: ranges are lists of Int.
  for k in 0..3 {
    i += k
  }
  i + line.qty
}

// No shared references: values are copied, so nothing changes behind your
// back. A declared type holds for a variable's whole life (every assignment
// is checked). Closures cannot change outer variables.

// ─── 9. Collections (all immutable: operations return new values) ──────────
fn collections(xs: NonEmpty[Int], text: String) -> Int {
  // xs[i] must be in range: checked at run time, proven by `verify` (here xs
  // is NonEmpty). get(xs, i) gives an Option instead. No negative indices.
  head := xs[0]
  maybe := get(xs, 5) ?? 0
  // Slices take a range and are clamped, as in Python; strings too.
  rest := xs[1..]
  prefix := text[..3]
  // Comprehensions, with `where` to filter.
  squares := [x * x for x in xs where x > 0]
  grid := [(a, b) for a in 1..=2 for b in ["x", "y"]]
  d := Dict { "tea": 450, "cup": 1200 }
  d2 := insert(d, "pot", 3000)
  price := get(d2, "tea") ?? 0
  lengths := Dict { w: len(w) for w in split(text, " ") }
  s := Set { 1, 2, 2, 3 }
  empty := Dict[String, Int] {}
  total := fold(xs, 0, (acc, x) -> acc + x)
  pairs := enumerate(zip(sort(xs), ["a", "b", "c"]))
  head
    + maybe
    + len(rest)
    + len(prefix)
    + len(squares)
    + len(grid)
    + price
    + len(lengths)
    + len(s)
    + len(empty)
    + total
    + len(pairs)
}

// ─── 10. Traits: what a type can do ────────────────────────────────────────
trait Priced {
  fn price(self) -> Natural
}

trait Describe {
  // A contract on a trait method binds every implementation.
  fn describe(self) -> String
    ensures len(it) > 0
}

// Supertraits, default methods, and laws (checked by `strata verify`).
trait Summary: Describe + Priced {
  fn summary(self) -> String {
    f"{self.describe()}: {self.price()}c"
  }
}

trait Combine {
  fn combine(self, other: Self) -> Self
  law associative(a: Self, b: Self, c: Self):
    a.combine(b).combine(c) == a.combine(b.combine(c))
}

// A method without `self` is called on a type: Money.empty(), T.empty().
trait Empty: Combine {
  fn empty() -> Self
  law identity(a: Self): Self.empty().combine(a) == a
}

type Item = { name: NonEmptyString, cents: Natural }

impl Priced for Item {
  fn price(self) -> Natural {
    self.cents
  }
}

impl Describe for Item {
  fn describe(self) -> String {
    self.name
  }
}

// Every method has a default here, so the block is empty.
impl Summary for Item {}

// A type's own methods (no trait), in the module that defines the type.
impl Item {
  fn new(name: NonEmptyString) -> Item {
    Item { name, cents: 0 }
  }

  fn priced(self, cents: Natural) -> Item {
    Item { ..self, cents }
  }
}

type Money = { cents: Int }

impl Combine for Money {
  fn combine(self, other: Self) -> Self {
    Money { cents: self.cents + other.cents }
  }
}

impl Empty for Money {
  fn empty() -> Money {
    Money { cents: 0 }
  }
}

// Generic impls, also for built-in types.
impl[T] Combine for List[T] {
  fn combine(self, other: Self) -> Self {
    self + other
  }
}

// Generic functions state their bounds.
fn combine_all[T: Empty](xs: List[T]) -> T {
  fold(xs, T.empty(), (acc, x) -> acc.combine(x))
}

// A trait used as a type means "any implementation": this is
// `fn receipt[T: Summary](x: T)`. Name the parameter only to tie things together.
fn receipt(x: Summary) -> String {
  x.summary()
}

// ─── 11. Capabilities are traits passed as parameters ──────────────────────
// No hidden effects: a database or clock is a trait, so the signature shows
// what code can touch. A method that changes its value takes `mut self`, and
// its calls are marked: `mut db.save(o)`.
trait Db {
  fn load(self, id: NonEmptyString) -> Option[Order]
  fn save(mut self, o: Order)
}

type ShipError =
  | NotFound
  | NotPending

fn ship(
  mut db: Db,
  id: NonEmptyString,
  tracking: NonEmptyString,
) -> Result[Order, ShipError]
  ensures match it {
    Ok(o) => o is Shipped,
    Err(_) => true,
  } {
  order := db.load(id) or { return Err(NotFound) }
  match order {
    Pending { id, lines } => {
      o := Shipped { id, lines, tracking }
      mut db.save(o)
      Ok(o)
    },
    _ => Err(NotPending),
  }
}

// ─── 12. Typed holes: unfinished code that still checks ────────────────────
// `?name` stands for an expression of the expected type. `strata holes` lists
// each with its goal type, context and obligations: a task for an agent.
fn refund(o: Order) -> Cents
  ensures it >= 0
  = ?refund_policy

// ─── 13. Termination ───────────────────────────────────────────────────────
// `verify` shows termination itself (a decreasing measure) or says
// "may not terminate". `terminates` after the contract assumes it.
fn collatz(n: Positive) -> Natural
  terminates {
  if n == 1 {
    0
  } else if n % 2 == 0 {
    1 + collatz(n / 2)
  } else {
    1 + collatz(3 * n + 1)
  }
}

// ─── 14. Tests ─────────────────────────────────────────────────────────────
test "orders" {
  l := Line { sku: "TEA-7", qty: 2, unit_price: 450 }
  assert line_total(l) == 900
  assert classify(-5) == "negative"
  assert explain("x") == "not a number: x"
  assert combine_all([Money { cents: 1 }, Money { cents: 2 }]) == Money { cents: 3 }
  assert Item.new("tea").priced(42).summary() == "tea: 42c"
  assert receipt(Item.new("cup")) == "cup: 0c"
}

test "a mocked capability" {
  // `mock Db` is an implementation whose methods return arbitrary valid values.
  mut db := mock Db
  r := ship(mut db, "ord_1", "TRK9")
  assert r is Ok || r is Err
}
```

## Rules that catch newcomers

- **Never write a `.strata` file yourself.** It is a binary module file: the syntax tree. Code goes in as text through an edit (`strata edit`, `strata add`, or `strata --file F edit < text` outside a package), which parses it, checks it and stores the tree; it comes out as text through `strata render` / `strata view`. How you lay the text out doesn't matter: it is printed back in one canonical style, the one the code above is in.
- **Every name a variant introduces is a type**, so it must be unique across the project: no other variant, type or trait anywhere may share it (`Empty` as a variant would clash with a trait `Empty`).
- **In one module, a function can't share a name with a trait method** (`fn describe` next to `trait Describe { fn describe … }`), since `x.describe()` could mean either. Across modules it can: `x.describe()` is the method, `describe(x)` the function.
- **Overloaded builtins can't be passed by name** (`len`, `get`, `contains`, `sum`…): write `xs.map(x -> len(x))`. Ordinary functions can: `lines.map(line_total)`. Methods whose meaning depends on the receiver also need a lambda: `xs.map(x -> x.describe())`.
- **`type Name = String` is an error**: a bare alias adds nothing. Add a `where`, or say `distinct type`.
- **Any function can be called as a method** on its first argument: `xs.filter(p)` is `filter(xs, p)`, `line.line_total()` is `line_total(line)`.
- **`mut` is visible on both sides**: `fn f(mut x: T)` and the call `f(mut x)`; `fn save(mut self, …)` and the call `mut db.save(o)` (required there, an error anywhere else). Only a `mut` variable, a `mut` parameter or a field of one can be changed, and a `mut` argument needs exactly the parameter's type. As an argument, a marked call takes parentheses: `f((mut q.pop()), x)`.
- **A declared type holds forever**: `mut x: Positive = 1` then `x = 0` is an error (at compile time for literals, at run time otherwise).
- **Matches are exhaustive and every arm must be reachable.** Conditions comparing `it` with constants are decided exactly; anything else (`matches(it, …)`, a call) needs a `_` arm.
- **`Result` and `Option` each have their own operators**: `!`, `except`, `!!` for a `Result`; `?`, `or`, `??` for an `Option`. Using one on the other type is an error. A single-value fallback is always written short (`x ?? 0`, not `x or { 0 }`); `??` and `!!` bind looser than arithmetic and tighter than comparisons.
- **No implicit conversions**: `Int` and `Float` don't mix, and there is no conversion between error types (`!` passes an error up only when it already fits the function's error type: the same type, one of its variants, or a member of its union).
- **Comments attach to what follows them.** A comment where nothing follows (after a block's last statement, inside an argument list) is an error, not silently dropped.
- **Empty literals need a type when nothing says it**: `let xs: List[Money] = []`, `Dict[String, Int] {}`.
- **Indexing is checked**: `xs[i]` faults when `i` is out of range, and `verify` tries to prove it isn't. There are no negative indices (`xs[len(xs) - 1]`); use `get(xs, i)` for an `Option`.
- **`print(x)` is for convenience.** Code whose output matters takes a `Console` (`std.io`), so tests can record it.
- Not in the language (yet): shared references, exceptions, null, implicit effects, iterators/generators, async, macros, inheritance.

## Modules and packages

A file is a module, named after its path in the package (`shop/orders.strata` is `shop.orders`; `main.strata` is the root module). A folder with a `strata.toml` is a package (`strata init`); a lone `.strata` file is a script. Definitions are private unless `pub`, and modules meet only through their public contract (`pub` types, signatures, contracts, laws).

```rust
// shop/orders.strata
pub type Order = { id: OrderId, lines: NonEmpty[Line] }

pub fn ship(mut db: Db, id: OrderId) -> Result[Order, ShipError] { ... }

fn helper() -> Int { ... }             // private to shop.orders

// billing.strata
use shop.orders.{Order, ship}          // or `use shop.orders.*`, or `shop.orders.ship(x)`
```

## Programs

`main` asks for what it needs by type and `strata run` passes it in: a command-line options record, and the capabilities `Console` (`std.io`), `Files` (`std.fs`), `Environment` (`std.process`). It returns `()`, an `ExitCode`, or a `Result` of either (an `Err` exits with 1).

```rust
use std.cli.CommandLine
use std.io.Console

/// Greet people by name.
type Options = {
  /// Shout the greeting.
  loud: Bool = false,                              // a flag: --loud
  /// Say it this many times.
  times: Int where 1 <= it && it <= 10 = 1,        // an option, as it has a default: --times 3
  /// Who to greet.
  names: List[String] = [],                        // the positional arguments
}

impl CommandLine for Options {}                    // --help comes from the /// comments

fn main(opts: Options, mut console: Console) {
  for name in opts.names {
    for _ in 0..opts.times {
      mut console.print(if opts.loud { to_upper(f"hello, {name}!") } else { f"hello, {name}!" })
    }
  }
}
```

Tests pass stand-ins instead of the real capabilities: `recorder(input)` / `printed(r)` for a `Console`, `mem_files([(path, text)])` for `Files`, `fake_environment(args, vars)`, or `mock Trait`. Nothing but `strata run` and `strata serve` reaches the real terminal, disk or network.

The standard library is written in Strata and needs no dependency: `std.io`, `std.fs`, `std.process`, `std.cli`, `std.http` (`Request`, `Response`, `Router`: `app.get("/users/:id", handler)`, `req.param("id")`) and `std.web` (`Html[M]` as values: `div([class("x")], [text("hi")])`, `on_click(msg)`). A web server defines `fn handle(req: Request) -> Response`; an app defines `init() -> Model`, `update(model, msg) -> Model` and `view(model) -> Html[Msg]` (the Elm architecture). Both run with `strata serve`. See `examples/hello`, `wc`, `page`, `counter`, `todos`. `strata builtins` lists every builtin with its signature.

## The tools

Every command works on the package around the current folder, or on a file or folder with `--file PATH`. Code is stored as binary `.strata` module files, so you read it through the views and change it through edits; `strata render --split DIR` writes a package out as `.st` text files, for reading only.

```sh
strata check                  # types, exhaustiveness, refinements of literals. Run it after every edit
strata test [FILTER]          # run `test "…" { … }` blocks
strata verify [NAME]          # prove contracts with Z3/cvc5, fuzz the rest, show termination
strata verify NAME --checks   # …listing every check and how it was settled
strata holes                  # each ?hole: goal type, what's in scope, what it must satisfy
strata run [PATH] -- ARGS     # run main
strata run FN '[JSON args]'   # call one function; args are checked against the types (--trace)
strata serve [PATH]           # run `handle` or an app (init/update/view) on http://127.0.0.1:8000
strata build [PATH] -o OUT    # native executable through LLVM (part of the language so far)
strata builtins               # every builtin and prelude type
```

`verify` gives each function one result: `✓` proven, `~` fuzzed (some checks unproven, no failure found; each says why), `✗` failed (with a counterexample that was run to confirm it), `○` incomplete (a hole), `≈` assumed. Proven doesn't imply it terminates: that is reported on its own line.

**Reading and editing a package.** Code is stored as one canonical file per module, but the tools work on definitions:

```sh
strata view --level 0                 # a map: one line per definition, deps, holes, status
strata view slice:Order --level 1     # Order, what it uses, what uses it: interfaces only
strata view 'ship,refund' --level 3   # full source of a few definitions (levels: 0 map, 1 interfaces, 2 outline, 3 source)
strata interface shop.orders          # a module's public contract
strata edit [MODULE] -m "why" < defs.strata
                                      # add or replace definitions by name; refused unless the
                                      # whole package still checks. --rm NAME deletes one
strata rename OLD NEW                 # functions, types, variants, trait methods, fields (Type.field)
strata move NAME MODULE               # to another module, imports follow
strata log / strata diff              # the change history, as structural diffs
strata ui                             # live viewer in the browser (http://127.0.0.1:7777)
```

An `edit` that breaks code elsewhere turns each broken use into a hole (`?pending_1`, …) and opens a pending change: fill them with more `edit`s, then `strata done` (or `strata abort`). In a git repository every recorded change is also a commit.

**Before pushing:** `strata ci` (everything checks, exports exist, every module file is exactly as strata writes it; `--fix` rewrites them). `strata setup-git` makes `git diff` show `.strata` files as text and merge them definition by definition. `strata sync` merges `main` into the branch definition by definition, applies the other side's renames, and commits only if `ci` passes.

**A good loop for an agent:** read with `view --level 1` and `holes`; write definitions; `check`; `test`; `verify` the functions you touched, and when one fails, read the counterexample, then fix the code or strengthen the contract (`requires`, a refinement) until it is proven.
