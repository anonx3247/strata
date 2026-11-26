# Language reference

This page holds the detailed rules of [the language](language.md). Each section adds to the section with the same name in the guide.

## Records and defaults

A default is checked where its type is defined. It must have the field's type, and a literal default must satisfy the field's refinement (`n: Positive = 0` is an error). A default sees no variables, not even the other fields, and it is evaluated each time it is used. Generated values (mocks, fuzzing) still generate every field.

**Key paths.** A field written after its record type's name, `Todo.title`, is a value of type `KeyPath[Todo, String]`. core defines `type KeyPath[R, T] = { name: String, get: (R) -> T }`: the field's name and a function that reads it, so `(Todo.title.get)(t)` is `t.title`.

- The key path's types come from the field, so code that takes one is checked against the record. In `std.orm`'s queries, `Todo.done.eq(false)` needs a `Bool` and gives a condition on `Todo`s only.
- A field the record doesn't have is an error (`type `Todo` has no field `x``).
- The checker expands it (`desugar.key_paths`) into `KeyPath { name: "title", get: ($r: Todo) -> $r.title }`. `view` still shows `Todo.title`, and nothing after the checker sees the short form.
- Not supported: fields of a generic record (`Pair.first`), paths through several fields (`Todo.owner.name`), and assigning through a key path.

**Spreading a record into a call.** The checker expands `f(a, ...x, ...y, b)` into the call written out by parameter name. Positional arguments before the spreads fill the first parameters, and those after the spreads fill the last ones. Each parameter in between takes the field with its name from the one spread record that has it (`x.qty`). Other fields are ignored. Everything after the checker (the compiler, `verify`, runtime checks) sees an ordinary call, while `view` shows the call as written. In the tree, a spread is `Spread(Expr)`, a call argument.

Spreads work in:

- calls to a function by name
- method calls on a value (`r.m(...x)`, `mut r.m(...x)`). For a trait value or a type parameter, the method is resolved through its trait.
- method calls on a type (`Shop.new(...x)`), where `self` counts among the parameters

Each of these is an error that names the call:

- a parameter that no record has a field for (parameters have no defaults)
- a parameter that two records both have (`is given by both ...x and ...y`). Spread only one of them, or pass that parameter positionally.
- a `mut` parameter left to a spread, since a spread only reads. Pass `mut v` positionally.
- a spread of a value that isn't a record (`...5`)
- a spread record that provides no parameter
- a positional argument between two spreads
- more positional arguments than parameters
- a spread into a function stored in a variable (its parameters have no names) or into a builtin
- a spread anywhere other than such a call, such as a contract or a constructor. `Pt(...p)` is a parse error, and `Pt { ..p, x: 1 }` is the record update syntax.

The spread value is read once. If it isn't a variable or a field of one (`...make()`), it is bound before the call's other arguments are evaluated. The same holds for a receiver that isn't a variable or field.

## Unions, variants and patterns

**A union's type parameters must be wrapped.** They can appear inside a member the union declares (`type Option[T] = Some(T) | None`, `type OrError[T] = Value(T) | Problem`) or inside a type (`type Many[T] = List[T] | Problem`). A type parameter can't be a member by itself (`T | Problem`), and two members can't differ only in their type arguments (`List[A] | List[B]`), because their values couldn't be told apart. A union of two types is written `A | B` where it is used; there is no `Either[A, B]`. A generic union is therefore identified by its members: `List[Int] | Problem` is `Many[Int]`, renders as `Many[Int]`, and `impl[T] Show for Many[T]` applies to it.

**A named union names a set of values.** A member fits the union without wrapping. The same set written out (`x: Circle | Square | Dot`) is the same type, and `strata view` writes it as `x: Shape`. Named subsets are used too: with `type A = B | C`, `B | C | D` is written `A | D`.

- A named union can have `impl`s, methods and `derive`s. A value typed by the union, or by its set, uses the union's own; a value of a member uses the member's.
- Two modules may name the same set (`geo.Shape`, `ui.Figure`), but they can't both implement the same trait, or define a method with the same name, for it. A value of the set wouldn't know which one applies.
- A union that adds nothing is rejected, like a bare alias. That covers a union that normalizes to a single type (`type U = Int | Positive`), and a second name for a set already named in the same module, in the root module or in the prelude (`type Answer = Some(Int) | None` is `Option[Int]`).
- `distinct type Answer = Some(Int) | None` is a separate type, built as `Answer(Some(3))` and read with `.value`.

`strata view` shows a type inline in the one union that lists it, unless the type has its own doc, `impl` or `derive`, in which case it is shown separately. `--members separate` shows every member separately. Both forms read back as the same program.

**Exhaustiveness and reachability.** The checker splits the matched type into variants, union members, and the ranges that the arms' bounds cut out of an `Int`, a `Float`, or the length of a `String` or `List`. It reports values that no arm matches (`` `where 3 <= it && it <= 5` is not covered``) and arms that earlier arms already cover (`unreachable match arm `Positive``).

Conditions that compare `it` or `len(it)` with constants, combined with `&&`, `||`, `!` and `==>`, are decided exactly. Any other condition (`matches(it, …)`, a call, another variable) is left to `strata verify`. A match that only such arms might complete is not a checker error. The prover proves that some arm matches, or shows a value that none does (`no match arm for 1`), and the compiled code checks it at run time (a `no match arm` fault).

**The matched value's refinement is taken into account.** With `fn f(n: Positive)`, `match n { 1 => …, where it > 1 => … }` is exhaustive, and a `where it <= 0` arm is unreachable (`no `Positive` matches it`). With `xs: NonEmpty[Int]`, `match xs { [x, ..] => … }` is exhaustive. This applies to variables (including mutable ones, since a declared type holds after every assignment), their fields, a function's result type, and the value inside a narrowing arm. Narrowing follows the code: an arm that assigns the matched `mut` variable anywhere does not narrow it, so the variable keeps its declared type in that arm. This also covers the next pass of a loop.

A field that every member of a union has can be read without a match. A trait method can be called on a union when every member implements the trait. Alternatives of different types (`if` branches, `match` arms, list elements) get their union type when no expected type says otherwise: `if big { Square {…} } else { Circle {…} }` is a `Square | Circle`.

**Type arguments are checked member-wise.** A `List[Circle]` can be passed where a `List[Shape]` is expected, because values are never shared and the callee works on its own copy. A function type fits when it accepts more and returns less. A `mut` parameter is the exception, because it writes back into the caller's variable: its argument must have exactly the parameter's type, since `grow(mut shapes: List[Shape])` could put a `Square` into a `List[Circle]`. For the same reason, `!` can pass an error into a wider error type (a `TooBig` into a `Result[_, ShapeError]`).

## Traits

**Mocks.**

- `verify` reports a mock's methods as assumed, since the tests that use the mock check them. `strata view` hides mocks and their implementations unless asked (`view --mocks`).
- A mock holds data in a record (`mock FixedClock = {}` for none).

**Capabilities.** std defines these capabilities, which `main` asks for by name:

- `Disk` (`std.fs`), implementing `Files`
- `Process` (`std.process`), implementing `Environment`
- `Launcher` (`std.process`), implementing `Commands`, for running other programs

`Commands` has one method, `run(mut self, program: String, args: List[String], dir: String) -> Result[Finished, String]`, where `Finished = { code: Int, out: String, err: String }`. It starts `program` (looked up in `PATH`) with `args`, never through a shell, in the folder `dir` (`""` for the current one), with an empty standard input. It waits for the program and returns its exit code (128 plus the signal number if a signal killed it) and its output. It returns `Err` only when the program can't be started (no such program or folder). Its `ensures` says the code is never negative, and that is all `verify` assumes about a call. Code that takes `Commands` is tested with `mock Commands`, or with a mock type that answers the calls the test expects.

`Console` is the one capability the environment chooses, so `main` asks for the trait. Under `strata run` it is the terminal. A test can ask for it too (`test "..." (mut console: Console) { ... }`). What the test writes is its own output, shown only if it fails. Under `strata run --trace`, the console's calls appear in the trace with what was written and read.

**The run mode** is hidden by default, so a program behaves the same everywhere. A package that needs it opts in through `strata.toml`:

```toml
[run]
mode = true
```

`main` and tests can then ask for a `RunMode` (`std.process`): `Running` under `strata run` or in a built executable, `Testing` in a test, and `Tracing` under `--trace`. Without the opt-in, asking for it is an error that explains how to allow it.

**Shared method names.** Two traits can declare methods with the same name, and each implementation keeps its own body. A call is resolved from what is known about the receiver:

- Known only through a trait (`d: Describe`, `T: Describe`): that trait's implementation.
- A known type: the type's inherent method if it has one (`impl Item { fn describe(self) … }`), otherwise the method of the one trait of that type that declares the name. This holds even when a function in another module has the same name. Inside your own `fn handle`, `app.handle(req)` calls `Router`'s method, while `handle(app, req)` calls the function.
- Anything else is a compile error. Examples: a type that implements two traits declaring `describe`, with no inherent `describe`, or a value known only as `Describe | Log` or `T: Describe + Log`. In these last two cases an inherent `describe` on a type doesn't help, because the call doesn't say which trait's meaning it wants.

This holds whichever modules the traits come from. With `Console` (`std.io`) and `Files` (`std.fs`) both imported, `mut console.write(s)` and `mut files.write(path, s)` each call their own trait's `write`. A method named like a builtin (`Files.remove`) doesn't hide the builtin: on a value whose type has no trait declaring the name (a dict, list, string, …), `d.remove(k)` and `remove(d, k)` are the builtin.

`strata` resolves each call during checking and records the result in the program it runs, so the same call always runs the same implementation. A name whose meaning depends on the receiver can't be passed as a bare function value (`map(describe)`). Pass a lambda instead (`x -> x.describe()`).

**`Self` versus the trait as a parameter type.** `fn bigger(self, other: Self)` takes a value of the receiver's own type, and `fn bigger(self, other: Shape)` takes any implementation. On a trait value (a `Shape` of any implementation), a method taking `Self` can only be given the value itself or a value made by its `Self`-returning methods (`s.bigger(s.scaled(2))`). `xs[0].bigger(xs[1])` is an error, since `xs[1]` may be a different implementation.

**Models: abstract state of a trait.** A trait can declare abstract state with `model name: Type`, one per line before its methods. Each implementation defines the model from its own state with `model name = e`, an expression over `self`.

- Contracts refer to a model as a field of a value of the trait: `self.orders`, or `db.orders` for `db: Db` or `db: D` with `D: Db`. `old` works on models too. Code never reads a model directly; it uses the trait's methods.
- `invariant e` in a trait, over `self`, refines every implementing type's model as `Self where e`. Every value of the trait satisfies it. Each method knows it holds for `self` and for any `Self` parameter, a `mut self` method must preserve it, and a method returning `Self` must establish it. A function taking `db: Db` knows it too.
- `ensures e` in a trait, which may use `old`, must hold after every `mut self` method.
- There is no trait-level `requires`.

```strata
trait Db {
  model orders: Dict[Int, Order]
  invariant !self.orders.contains(0)

  fn save(mut self, id: Int, o: Order)
    ensures self.orders == old self.orders.with(id, o)
  fn lookup(self, id: Int) -> Option[Order]
    ensures it == self.orders.get(id)
}

impl Db for MemDb {
  model orders = self.all

  fn save(mut self, id: Int, o: Order) {
    mut self.all.insert(id, o)
  }
  ...
}
```

`d.with(k, v)` and `d.without(k)` on a dict, and `s.with(x)` and `s.without(x)` on a set, build new values for use in contracts. Three more state common contracts directly:

- `d.excluding(keys)`: `d` without the entries of `keys` (a list or a set). "Nothing but these entries changed" is `self.orders.excluding(ids) == (old self.orders).excluding(ids)`.
- `d.key_set()`: the keys of `d` as a set (`d.key_set().contains(k) == d.contains(k)`).
- `xs.unique()`: no element twice (`len(set(xs)) == len(xs)`).

`verify` reads them as the solver's own operations: `excluding` is the dict's array with those keys masked, so a frame needs no quantifier over the entries; `key_set` is the array of the keys whose entry is present; `set(xs)` of a list holds exactly its elements, and `unique` says position to element is one to one.

`verify` proves each implementation's methods against the trait's contracts, reading the model through the `model` definition. Code that takes the trait is proven through the model alone ([verification](verification.md#trait-models)). A contract that names a model is assumed by default: `verify` proves it, but it is not checked at run time, since that would compute the model on every call. `run --check-assumed` and `test --check-assumed` check it.

An `impl` must define every model of its trait and no others. A mock type's `impl` may omit them.

`mock Db` for a trait with models simulates the models. It stores each one, starting empty (`{}`, `[]`, `0`, `""`, `false`, `None`). A method whose `ensures` states the new value of a model (`self.orders == e`) and its result (`it == e`) does exactly that. Any other method returns a generated result that satisfies its contract, as with every mock. `mock Db` is therefore an in-memory reference implementation for tests.

**Laws made of statements.** A law is a `Bool` that every implementation must satisfy (`law associative(a: Self, b: Self, c: Self): …`). It can also be a block of statements, including `mut` method calls on copies of its parameters, that ends in the `Bool` that must hold:

```strata
law enqueue_once(q: Self, id: Int) {
  mut r := q
  mut r.enqueue(id)
  (mut r.enqueue(id)) == 0
}
```

`verify` proves the law for each implementation, through its code or through its methods' contracts, assuming its `Self` parameters meet the trait's invariants. A counterexample gives the values and the steps: `law `claim_after_enqueue` does not hold for `MemQueue` with q = MemQueue { waiting: Set[Int] {}, taken: Set[Int] { 21 } }, id = 21: after mut r := q; mut r.enqueue(id); mut r.claim(id)`.

A law describes what one worker sees when it makes those calls in a row with nobody else in between. Callers of the trait can't use a proven law as a fact. They reason only through the methods' contracts and the invariants.

**`shared` traits: many workers, one value.** `shared trait Queue { … }` declares a trait whose value many workers use at once. It models a backend whose state is outside the program, such as a database or a remote queue, with each worker holding a handle to it.

- Each method is one atomic step on the model. Every `mut self` method preserves the trait's invariants from any state where they hold, so the invariants hold however the workers' calls interleave.
- Callers can't keep facts about the models from one call to the next, since another worker may act in between. Before each call on a value of a shared trait (`q: Queue`, `Q: Queue`), `verify` treats the model as any value the invariants allow. The call's `requires` must therefore follow from its arguments and the invariants alone. Its `ensures` relates the state just before and after that call, and what it says about the model holds until the next call on the value. The call's result keeps its facts: `a := mut q.enqueue(id)` is 0 or 1 whatever happened before.
- An implementation's method must be one step. On every path it changes its outside state with at most one call. Outside state means fields whose type is a capability, a `capability trait` or another shared trait. A read followed by a write (`self.db.select(id)`, then `mut self.db.update(id, 1)`), or two writes, is an error: "another worker's call can run between `select` and `update`". A method taking `mut self` counts as a write, one taking `self` as a read, and every capability function as a write, since it reaches the outside world. Several reads are allowed, and so is a read after the write. A function passed into the one call (`mut self.db.transaction(id, s -> …)`) runs inside that step, so a transaction-style method is how an implementation reads and writes together.

**Trait methods without `self`** are called on a type, never on a value: `Money.empty()`, `T.empty()` in a generic function, `Self.empty()` in a trait's default methods and laws. They are constructors and similar functions (`fn empty() -> Self`, `fn parse(s: String) -> Result[Self, ParseError]`). Their signature must mention `Self`; one that doesn't couldn't select an implementation and should be a plain function.

In a generic impl's methods, they can be called on the impl's type parameters too: `impl[T: Zero] Zero for List[T] { fn zero() -> Self { [T.zero()] } }`, or `fn padded(self, n: Int) -> Self { .. push(out, T.zero()) .. }` in `impl[T: Zero] Pad for List[T]`.

Types are passed whole at run time. The type a method is called on (`List[Int]`), or the static type of the receiver, determines `T`. Only a value held as a trait type (`d: Describe`) doesn't carry its full type.

A refined alias stays itself as a type argument. In `read[List[Port]]`, `T` is `Port`, whose methods are `Int`'s. The same holds for an alias with type parameters (`type Many[T] = List[T] where len(it) >= 2`, where `Many[Int]` has `List`'s methods) and for a refinement written as a type argument (`List[Int where it % 2 == 0]`, shown as written in messages).

**`T.check(x)`** works on any type, including a type parameter (checked against its run-time type). It returns `Ok(())` when `x` satisfies the type's refinements, and otherwise an `Err` describing what fails: `Port.check(0)` is `Err("0 does not satisfy `Port` (where 1 <= it && it <= 65535)")`.

Because the type is always named, it is always clear which implementation runs. A generic function that calls such a method receives its type arguments at run time: each call passes the concrete type the checker inferred, or forwards the caller's own type parameter. The type must therefore be known at each call. `combine_any([])` alone is an error (annotate it: `let xs: List[Money] = []`), and such a function can't be passed as a value (`map(combine_any)`); use a lambda. A trait-typed parameter (`xs: List[Empty]`) has no type name to call `.empty()` on, so write the type parameter when you need one.

**Writing a `derive`.** `derive A, B for`, written between a declaration's `///` docs and the declaration itself (`pub`, then `fn` or `type`), derives each trait for it:

```rust
/// A greeting for `name`.
derive C, Python for
pub fn greet(name: String) -> String {
  f"Hello, {name}!"
}

derive CommandLine, Json for
type Options = { count: Int, loud: Bool, name: String = "world" }
```

- This is only syntax. Each trait's derive is stored as its own definition (`derive CommandLine for Options`, `derive Json for Options`), exactly as the separate form `derive Json for Options` stores it. `derive A, B for T`, where `T` is a type, a function or `module`, also gives one definition per trait.
- `strata view`, the text `strata edit` opens, and `strata docgen` show a derive on its declaration when it has no hand-written block of methods, no type parameters or trait arguments, and no comments of its own, and the declaration is in the same module. All of a declaration's such derives go on one line, in their order in the module.
- Other derives stay on lines of their own: one with a block (`derive Json for Point { ... }`), one for a type in another module, and `derive C for module`. A derive with a block names a single trait.
- An edit replaces and adds derives by name, like any definition. A declaration given in an edit brings its derive line. A stored derive that would be shown on that line but whose trait the line no longer names, or whose line is missing, is removed with a warning (`warning: removed derive std.toml.Toml for Options (no longer on its derive line)`).
- A declaration the edit doesn't give keeps its derives. Derives with a block, derives for another module's declaration, and `for module` derives are removed only with `strata edit --rm 'derive std.json.Json for Options'`.
- `strata view Options`, and the editor `strata edit Options` opens, show the declaration with its derive line, so editing what they show keeps the derives.

## Modules

- Each `.strata` file in a folder (a binary module file) is a module named after its path: `shop/orders.strata` is `shop.orders`, and `main.strata` is the root module.
- `view MODULE` prints one module as text, and `view --split DIR` writes the codebase as one `.st` file per module. A view that spans several modules (`view a,b`) labels each group with a `// ── module m` comment.
- In the database, definitions are stored under qualified names, so `orders.Line` and `billing.Line` can coexist. `edit m` targets a module, and `view module:m` shows one.
- `examples/tasks/` has three modules (`model`, `memory`, `print`) that use each other only through their contracts.

## Mutation

Only a `mut` variable, a `mut` parameter, or a field of one can be changed, either by passing it `mut` or by marking it as a receiver. A call's result is a value, not a place, so its changes would be lost: `mut first(xs).tick()` is an error. Bind it first (`mut x := first(xs)`).

A variable's declared type holds for its whole life. Every assignment is checked against it: `line.qty = 120` fails if `qty` is a `Quantity`, and `x = -1` fails if `x` was declared `mut x: Positive`. Literals are checked at compile time and other values at run time. The value that a `mut` argument writes back is checked too. Passing a `Positive` variable to a `mut k: Int` parameter is a compile error, because the function could store any `Int` in it. Closures can't change outer variables.

A place passed `mut` is read after the call's other arguments have run. In `both(mut g, bump(mut g))`, `bump` changes `g` first, and `both` receives `g` as `bump` left it. The same goes for a receiver (`mut g.add(bump(mut g))`) and for an argument block that assigns `g`. Passing the same variable `mut` twice in one call (`swap(mut g, mut g)`) is an error, since each would write back its own value and one would undo the other.

**Changing lists, dicts and sets.** A list, dict or set held by a `mut` variable, a `mut` parameter or a field of one changes through its `mut` methods, marked at the call like any method that changes its receiver. Expressions build new values. Each operation has one form:

| Change in place | Cost (when nothing else holds it) | Contract (`ensures`) | New value instead |
|---|---|---|---|
| `mut xs.push(x)` | O(1) amortized | `xs == old xs + [x]` | `xs + [x]` |
| `mut xs.pop()` → `Option[A]` | O(1) | `xs == old xs.take(len(xs) - 1)` | `xs[..len(xs) - 1]` |
| `mut xs.insert(i, x)` (`0 <= i <= len(xs)`) | O(n) | `xs == old xs.take(i) + [x] + old xs.drop(i)` | `xs[..i] + [x] + xs[i..]` |
| `mut xs.remove(i)` (`0 <= i < len(xs)`) | O(n) | `xs == old xs.take(i) + old xs.drop(i + 1)` | `xs[..i] + xs[i + 1..]` |
| `mut xs.clear()` | O(n) (each element dropped) | `len(xs) == 0` | `[]` |
| `mut xs.sort()`, `mut xs.sort_by(key)` | O(n log n) | `xs == old xs.sorted()` | `xs.sorted()`, `xs.sorted_by(key)` |
| `mut xs.reverse()` | O(n) | `xs == old xs.reversed()` | `xs.reversed()` (strings too) |
| `mut d.insert(k, v)`, `mut d.remove(k)` | O(1) average | `d.get(k) == Some(v)`, `!d.contains(k)` | a changed copy: `{ mut e := d; mut e.insert(k, v); e }` |
| `mut s.insert(x)`, `mut s.remove(x)` | O(1) average | `s.contains(x)`, `!s.contains(x)` | `s.union(Set { x })`, `s.difference(Set { x })` |
| `mut d.clear()`, `mut s.clear()` | O(1) | `len(d) == 0` | `Dict[K, V] {}` |

- If something else holds the value (a copy in another variable, a `for` over it), the first change copies it once ([values and copying](language.md#values-and-copying)).
- The methods return `()`, except `pop`, which returns the removed element. That element is a value like any other, so dropping it is an error; write `_ := mut xs.pop()`.
- Calling a method without the `mut` marker (`xs.push(3)`) or as a function (`push(xs, 3)`) is an error that shows the correct form. `strata fix` rewrites such code: `xs = push(xs, x)` becomes `mut xs.push(x)`, and `ys := sort(xs)` becomes `ys := xs.sorted()`.
- The receiver is a variable or a field of one. An element is assigned whole (`grid[r] = grid[r] + [x]`).
- `verify` models each operation exactly, with dicts and sets as the solver's arrays. An index out of range is a proof obligation, as for `xs[i]`.

**Ranges and slices.** A slice is clamped to the length, as in Python. Finite iterators, such as a string's units, slice the same way: `s.graphemes()[..3]`. There are no negative indices; the end of `xs` is `len(xs)`, as in `xs[..len(xs) - 1]`. `xs[i]` requires `i` to be in range. This is checked at run time and proven by `verify`: `xs[0]` on a `NonEmpty[Int]` is proven, while `xs[len(xs) - 1]` on an arbitrary list is not. `get(xs, i)` returns an `Option` instead.

**Assigning an element.** On a list that can be changed (a `mut` variable, a `mut` parameter, or a field of one), `xs[i] = v` means `xs = set_at(xs, i, v)`: `xs` with element `i` replaced by `v`, the other elements and the length unchanged.

- `i` has the same condition as in `xs[i]`, `0 <= i && i < len(xs)`. It is a fault at run time and an obligation for `verify`, and the check is left out of the build once proven.
- Places nest. `s.xs[i] = v` is `s.xs = set_at(s.xs, i, v)`. `grid[r][c] = v` is `grid = set_at(grid, r, set_at(grid[r], c, v))`. `xs[i].f = v` changes the field in a copy of the element and stores it back.
- `+=` and the other compound assignments read the element at the same index. An index that isn't a variable or literal is evaluated once.
- When the element type is refined (`mut xs: List[Positive]`), only `v` is checked against it, not the whole list.
- It is O(1). Compiled code replaces the element in place when nothing else holds the list, or, for a field, the record. For `grid[r][c]`, the same applies to the row: only a shared row is copied, in O(its length). Otherwise the list is copied first (O(n)), as with `push` and `insert`.
- A slice (`xs[a..b] = …`), a unit of a string, and a dict key (`d[k]`, which can't be read either) can't be assigned.

Ranges, slices and comprehensions are stored in the tree as written, so they render back as written. Name resolution turns them into the builtins (`range`, `slice`, `map`, `filter`, `fold`), so the checker, compiler and prover treat them exactly like those calls.

## Functions that allocate nothing: `noalloc`

Allocation is implicit: values are reference-counted and freed when the last holder releases them. A function can be marked `noalloc` after its contract, next to `terminates`. `strata check` then rejects it if it might allocate, naming what allocates and where (`` `sum` is `noalloc` but allocates: a list made at run time (`[a, b]`) ``). `terminates` is assumed, but `noalloc` is checked.

```strata
fn total(xs: List[I64]) -> I64
  noalloc {
  mut acc: I64 = 0.to_i64()
  for x in xs {
    acc += x
  }
  acc
}
```

The check covers the body and the contracts, including loop invariants, since contracts run when the function runs. An `assumed` contract never runs, so it is skipped. The following allocate:

- **Building a value at run time:** a list, tuple, record or variant with fields (`Point { x, y }`, `Some(x)`), a dict, set, closure, f-string or comprehension, a big integer literal, a task (`spawn`, `scope`, `await`, `select`, `race`), a `mock`, `?` and `except`. A value built only from literals is a constant, built once, and doesn't count.
- **Arithmetic that builds a value:** `+ - * / % <<` and negation on `Int`, because results past 2^62 become big integers (compute on `I64` or another sized integer instead), and `+` on strings and lists. Arithmetic on sized integers and `Float` is allowed.
- **Updates that are only sometimes in place:** `xs[i] = v`, `x.f = v`, `push`, `pop`, `insert`, `remove` and the other `mut` methods of lists, dicts and sets. They update in place when nothing else holds the value, but that is known only at run time (a value passed in may be shared), and otherwise they copy. The check assumes nothing about a value's uniqueness.
- **Calls** to any function or method that isn't `noalloc` itself (a program's functions and methods by name; a trait method is `noalloc` when it is declared so and so is every implementation), to function values (parameters, closures), and to builtins other than those that never allocate: `len`, `byte_len`, `byte_at`, `at` (`xs[i]`), `ord`, `contains`, `starts_with`, `ends_with`, `is_alpha`, `is_digit`, `is_space`, `compare`, `min`, `max`, `sqrt`, `float`, and the conversions to sized integers (`to_i64`, `wrap_u8`, …). A builtin's name called as a method means the builtin on a list, string, dict, set or number, and the type's own method on any other type, which must then be `noalloc`.
- **A `for` over anything other than a list or `range(a, b)` (`a..b`):** a set or dict is turned into a list, and an iterator's `next` builds each `Option`. A `for` over a list borrows its elements, and a `for` over a range counts. `a..=b` is `range(a, b + 1)`, and that `+` on `Int` counts.

Reading is free: fields, elements, `match`, comparisons, `if`, `while`, `for`, `return`, string literals and constants. `xs[i]` faults when out of range, and a fault ends the program or the task, so it isn't counted. Faults and failed contract checks don't count either. Values the code generator places on the stack still count (a constructor that doesn't leave its block, an argument the callee doesn't keep), because where it does so depends on the whole program, and `noalloc` doesn't rely on it.

An `extern c fn` can be marked `noalloc`. Like `terminates`, that is the C code's promise and isn't checked.

**A whole module** can be declared `noalloc` in its package's `strata.toml`: `noalloc = ["fast", "mem"]` lists modules by their names in the package. A module includes the modules below it, `main` is the root module, and `*` means every module.

- Every function in those modules (plain functions, `impl`, trait and capability methods, `extern` functions) is `noalloc` without being marked, including functions added later.
- A function there that allocates is an error from `check`, `edit` and `verify`, naming the function and what allocates.
- Calls into those modules, from anywhere, are calls to `noalloc` functions.
- A name in the list that isn't a module of the package is an error.

core's own `strata.toml` declares its runtime modules (core.rt, the allocator, and the modules it uses) and core.mem this way.

## Lemmas and proofs: `lemma`, `assert … by`, `calc`

A `lemma` states a fact for `verify` to prove once and for proofs to use. It is written like a function with no result: its `requires` are its hypotheses, its `ensures` is the fact, and its body (optional) holds the proof's steps. It is never compiled, and only proofs call it. A recursive lemma is a proof by induction, so it must be shown to terminate.

```rust
lemma double_le(a: Int, b: Int)
  requires a <= b
  ensures a + a <= b + b
```

The statements of a proof:

- `assert e by lemma(args)` and `assert e by { steps }`: `e` is proven from the steps alone, and is then known. The steps are lemma calls, `assert`s, nested `assert … by`, `if` and `match` case splits, and `calc`s.
- `calc { a == b; _ <= c }`: a chain of steps, each proven alone. Each `_` is the right side of the step before it. The chain gives `a <= c`.

`lemma`, `by` and `calc` are contextual keywords: `lemma` before a definition's name, `by` after an `assert`'s condition, and `calc` before `{` at the start of a statement. Proofs compile to nothing, and views hide them by default. See [Verification](verification.md#lemmas-and-written-proofs).

## Errors

- Each operator works only on its own type. `except` and `!!` on an `Option`, or `or` and `??` on a `Result`, is an error. A `Result[Option[T], E]` therefore reads one way: `(load(k) !! None) ?? default` handles the error, then the missing value.

## Tasks

A task handle stays in its scope. A scope can't return one, a task can't hold another task's handle, and a handle can't be stored in a variable declared outside the scope.

A fault in a task (an index out of range, a broken contract) ends the program immediately, as anywhere else. Nothing catches it, and the other tasks stop too, so a thread waiting for what the failed task would have sent doesn't hang. The call stack shows where the task was started and where it failed: `main -> spawn -> pick`. Under `strata test`, the fault fails only that test: the test's other tasks stop at their next wait, and the next test runs.

**A deadlock is a panic.** A deadlock is reached when every task is waiting (in a `recv`, `send`, `select`, `await`, at the end of a `scope`, or in a virtual clock's sleep) and no wait can end on its own. A real clock's sleep or `after` timer can end on its own, so it doesn't count. The program stops and reports what each task is waiting on, and where: `deadlock: every task is waiting, so none can go on: main waits in the end of a `scope`, at main; a task waits in `recv`, at main -> spawn -> Receiver.recv; …`. Under `strata test`, the test fails with that report. A task reading the console or a file isn't waiting in this sense, since it can proceed on its own.

A `scope` whose last line is a `spawn` (`scope { spawn work() }`) evaluates to `()`, not the handle. Each task runs on its own thread.

**Channels.**

- Closing is explicit, once per task. Each task holding the sender (the one that created the channel and every `spawn` that uses it) calls `tx.close()` when it has finished sending, and the channel ends when all of them have. The checker requires the call where the channel is created and in each task that uses the sender. If a task still ends without closing, for example because the call was in a branch not taken, its copy is closed for it and that is a fault. A `spawn` can take a sender only as its own variable, not inside a tuple or record.
- Any number of tasks can send to and receive from one channel. `Sender[T]` and `Receiver[T]` are the types of the two ends.

**`race`.** A cancelled task stops at its next wait (a `recv`, `send`, `await`, `select`, `sleep`, or a console's `read_line`), however deep in the call stack, and unwinds as `return` would. What it holds is dropped, nothing after the wait runs, and the tasks it started stop too. Code that doesn't wait runs to its end, so a long computation isn't interrupted halfway. The `race` ends once every loser has stopped, then runs the winner's arm. Each left side follows `spawn`'s rules: no `return`, `!` or `?` out of it, and a sender it uses is its own copy, which it must close.

**Supervised tasks.**

- A handler after `await h` applies to the result: `await h except p { … }` is `(await h) except p { … }`, and `await h!` is `(await h)!`.
- When a supervised task panics, every task below it stops at its next wait. Its copies of senders are closed, so receivers see the channel end. What it held is freed, including after a division by zero or a bad builtin argument. Only a stack overflow or running out of memory leaves its memory allocated. Its siblings and the task that started it carry on. A panic in an ordinary task below it counts as the supervised task's panic.
- A `race` arm can be supervised the same way: `race { supervised fetch(a) as r => …, … }`. `r` is then a `Result[T, Panic]`, and a panic is that arm's result.
- `supervised` is a keyword only directly before `spawn` and at the start of a `race` arm. It is an ordinary name anywhere else.

**Time.** In a `select`, `clock.after(d) => …` is an arm that fires once `d` has passed on that clock since the `select` began. It binds nothing: `select { jobs.recv() as j => run(j), clock.after(ms(100)) => flush() }`. Any `Clock` works, so a test can pass a clock whose time it controls (one that stands still, or runs faster), and its `after` arms and `sleep` follow that time.

A virtual clock's sleeps end in deadline order, each once everything else is waiting. Timer tests are therefore exact and take no real time.

std has no stopped clock, so a test that needs one writes its own mock of `Clock`. It takes a few lines:

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

| Type | Notes |
|---|---|
| `Int`, `Float`, `Bool`, `String` | `Int` is arbitrary-precision, like Python's: a machine integer while it fits and a big integer when it doesn't, literals included. `Float` is never NaN or infinite; an operation whose result would be is a fault (`1e308 * 10.0`). Where that can't be ruled out, for example in code that runs other programs' arithmetic, `checked_add`, `checked_sub`, `checked_mul`, `checked_div`, `checked_rem` and `checked_sqrt` return an `Option[Float]`, and `checked_floor`, `checked_ceil` and `checked_round` an `Option[Int]`, with `None` in place of the fault. Ints and Floats never mix implicitly (`float(n)`, `round(x)`) |
| `Real` | exact rationals, core.real's record `{ num: Int, den: Positive }` in lowest terms. The checker refuses one built from its parts (`Real { num: 2, den: 4 }`) or a part assigned outside core.real, so `==`, hashing and `Dict` keys are structural. The checker writes each decimal literal as `Real { num: 1, den: 10 }` (from the `Float`'s shortest digits, so a literal of more than 17 significant digits is read as its nearest `Float` first) and each integer literal where a `Real` is wanted as `Real { num: n, den: 1 }`. The code generator makes a literal a constant block and calls core.real's `real_add`, `real_mul`, `real_cmp` … for the operators, a result of one put on the stack or into the variable it is assigned to; core.real has no call-stack frames. A division checks its divisor ("division by zero") unless `verify` proved it nonzero. `verify` encodes a `Real` as an SMT `Real` (`floor` is `to_int`) everywhere but in core.real, whose own functions see the record and are proven. A decimal literal pattern is `where it == 0.5`, written back as `0.5`. `%` is Euclidean, as on `Int`. `{x:.2}` rounds ties to even, as on `Float`. The fuzzer makes a `Real` as an `Int` over a `Positive`, reduced. `std.json`'s and `std.toml`'s `Number` hold a `Real` (beside `Integer(Int)`): read exactly, written with the shortest digits that are exact (`Real.number_text`); writing a `Real` with no terminating decimal (`1/3`) is an error, and `-0.0` reads as `0.0`. `std.sqlite` stores a `Real` as its text (`Row.real` reads it back), the ORM's `Real` columns are `TEXT` |
| `I8` … `I64`, `U8` … `U64` | Fixed-width machine integers, each a separate type compiled to its own width (`U32` is an LLVM `i32`). Arithmetic wraps: each operation (`+ - * / % & \| ^ << >>`, `-x`) computes the `Int` result, then takes it modulo 2^width (two's complement for the signed types). `/` and `%` are Euclidean, as on `Int`. A shift by the width or more gives 0, or -1 for `>>` of a negative number. They never mix with `Int` or with each other: `x + n` with `x: U32` and `n: Int` is an error. Convert with `x.to_int()`, `n.to_u32()` (`n` must fit; `verify` proves it or a run-time check enforces it) or `n.wrap_u32()` (`n` modulo 2^32). An integer literal is an `Int` unless its context makes it a sized integer: an annotation (`let b: U8 = 200`), a parameter, a result, a field, a list of them, or the other operand of an operator (`x + 1`, `x >> 3`). It must fit (`let b: U8 = 300` is an error). `verify` models them as integers in their range with every operation wrapped, so it finds where an `ensures` breaks on overflow (`x + 1 > x` fails for `x = 255`) |
| `()` | the unit type, for "no meaningful value". It plays the role of Python's `None` as a type |
| `Option[T]` | `None` or `Some(x)`: a value that may be absent |
| `Result[T, E]` | `Ok(x)` or `Err(e)`: the outcome of an operation that may fail |
| `List[T]` | an ordered sequence backed by a growable array, `[1, 2, 3]`. Changed with `mut xs.push(x)`, `pop`, `insert`, `remove`, `clear`, `sort`, `sort_by`, `reverse`. `xs.sorted()` and `xs.reversed()` return new lists |
| `(A, B, …)` | tuples, `(word, 3)`, destructured with `(w, n) := …` and accessed with `t.0` |
| `Dict[K, V]` | a hash map: `Dict { "a": 1, "b": 2 }`, `Dict[String, Int] {}`, `dict(pairs)` from a list of pairs. `get`, `keys`, `values`, `items`. Changed with `mut d.insert(k, v)`, `mut d.remove(k)`, `mut d.clear()`. A JSON object when the keys are strings |
| `Set[T]` | a hash set: `Set { 1, 2 }`, `Set[Int] {}`, `set(xs)` from a list. `union`, `intersection`, `difference`, `to_list`. Changed with `mut s.insert(x)`, `mut s.remove(x)`, `mut s.clear()` |

`+` adds numbers and concatenates strings and lists (`"a" + "b"`, `[1] + [2]`).

**Bit operations.** `&`, `|` and `^` work bitwise on `Int`s as infinitely sign-extended two's complement, as in Python: `-1 & x` is `x`, big integers included. `a << n` is `a` times 2^n, and `a >> n` is `a` divided by 2^n, rounded down (`-5 >> 1` is `-3`). A negative `n` is a fault, which `verify` looks for. Precedence follows Rust and Python, all tighter than comparisons: `|`, then `^`, then `&`, then the shifts, then `+` and `-`. So `x & 255 == x` is `(x & 255) == x`. The printer parenthesizes a bit operation inside a comparison, `(x & 255) == x`, because C, and so most readers, binds `==` first. In a refinement or a `where` pattern, where `|` separates union members or pattern alternatives, a bitwise or must be parenthesized (`where (it | 1) == 3 | 0`).

**Strings** are UTF-8. A string isn't counted or indexed directly; the unit is always named: `s.bytes()`, `s.code_points()` or `s.graphemes()`. These are iterators (`len(s.graphemes())`, `s.graphemes()[i]`; see Iterators), and `len(s)`, `s[i]` and `s[a..b]` produce errors that say so.

- Positions in a string are byte offsets. `index_of(s, sub)` returns one, and `byte_slice(s, a, b)` cuts at byte offsets in constant time. The offsets are clamped to the string, and an offset inside a character moves back to the character's start. `byte_len(s)` is the length in bytes.
- String functions don't fail. `split(s, "")` splits at every character (`["", "a", "b", ""]`), `replace(s, "", x)` puts `x` around every character, and `repeat(s, n)` with `n < 1` is `""`.
- Other string functions: `chars`, `contains`, `starts_with`, `ends_with`, `split`, `join`, `lines`, `trim`, `replace`, `repeat`, `reverse`, `to_upper`, `to_lower`, `is_digit`, `is_alpha`, `is_space`, `ord`, `chr`, `parse_int`, and `matches` (regex).
- JSON is provided by `std.json`. A type that derives `Json`, or a built-in type, is written with `x.to_json()` and `stringify`.
- `float_bits` and `float_from_bits` convert a float to its IEEE 754 bits as an `Int`, and back. `float_from_bits` returns `None` for a NaN's bits.

**f-strings** interpolate any expression, formatted by `to_string` (`Display`, below): `f"{name} has {len(xs)} items"`. `{{` and `}}` are literal braces. A **format spec** follows a `:`, as in Python: `[[fill]align][+][0][width][.precision]`.

- `{price:.2}` gives two decimal places, or keeps at most 2 characters of a `String`.
- `{name:<12}`, `{n:>5}` and `{title:*^20}` pad to a width. Numbers align right by default and everything else left.
- `{n:05}` pads with zeros after the sign.
- `{delta:+}` always shows the sign.
- A spec that doesn't suit the type is a check error.

**Showing values: `Debug` and `Display`.** Every type implements both traits implicitly.

- `Debug` (`debug(x)`, `x.debug()`) writes a value as code: `Some("hi")`, `Point { x: 1, y: 2 }`, `[1, 2]`, `(1, "a")`, `Dict { "a": 1 }`, `Set { 1, 2 }`, `UserId(7)`, with strings quoted.
- `Display` (`to_string(x)`, `x.to_string()`) is what `print` and f-strings show. A `String` displays as its text and anything else as its `debug` form. Parts inside a list, record or other value are shown with `debug`, as Python's `str` of a list uses `repr`, so `["a b"]` and `["a", "b"]` stay distinct.

A type can replace either with its own impl, which is then used everywhere the type is shown, nested values included:

```rust
impl Display for Money { fn to_string(self) -> String { f"${self.cents / 100}.{self.cents % 100:02}" } }
impl Debug for Secret { fn debug(self) -> String { "Secret { key: *** }" } }
print(Money { cents: 1234 })       // $12.34
print([Money { cents: 5 }])        // [Money { cents: 5 }]: parts by their `debug`
print(Some(Secret { key: "k" }))   // Some(Secret { key: *** })
```

**Order.** `compare(a, b)` is the order `sort` uses, on any two values of the same type. It returns -1 when `a` comes first, 0 when they are equal, and 1 when `b` comes first. Generic code, including `std.collections`, orders by `compare`, because `<` and the other comparison operators work only on numbers. The order is:

- numbers by value, strings by bytes
- a `distinct` value as its underlying value
- variants in declaration order, then field by field
- tuples, records and lists element by element, with a shorter list first

`xs = take(xs, n)` (or `o.f = take(o.f, n)`) changes the list in place when nothing else holds it, like `xs[i] = v`. It drops the elements past `n`, so a list used as a stack or heap shrinks without a copy.

All values are immutable. A `mut` variable changes through its methods (`mut d.insert(k, v)`, `mut s.remove(x)`), and a copy held elsewhere keeps the old value.

Dicts and sets are hash tables with no defined order. Iterating one (`for`, `keys`, `values`, `items`, `to_list`) or printing it gives the entries in table order, which may change whenever the implementation does. Nothing should depend on it; sort the keys when order matters. Hashing is deterministic, so a program behaves the same on every run. Equality is by content (`Dict { "a": 1, "b": 2 } == Dict { "b": 2, "a": 1 }`). Where dicts or sets are themselves ordered (`<`, `sort`, a set of sets), they compare as their sorted entries.

`len`, `contains`, `get`, `insert`, `remove`, `sum`, `min`, `max` and `abs` work on every type where they make sense. `strata builtins` lists every builtin with its signature. `strata view NAME` (`view insert`, `view Dict`) shows one with its doc, which ends with its cost: "Cost: O(1) amortized in place: in the statement `d = insert(d, k, v)` when nothing else holds `d`; O(n) otherwise (a copy ...)". See `examples/builtins.strata`.

**Iterators.** A `for` loop over an iterator works on its own copy of the iterator, so `c` is unchanged afterwards. It supports `break`, `continue` and patterns like any `for`. Generic code asks for the trait: `fn collect[T, I: Iterator[T]](it: I) -> List[T]`. `verify` can't prove such a loop terminates, as it can for a list, because the iterator may never return `None`.

An iterator that always ends also implements `FiniteIterator`, a subtrait whose `len` is the number of elements `next` will still return:

```rust
trait FiniteIterator[T]: Iterator[T] {
  fn len(self) -> Natural
  fn fold[B](self, init: B, f: (B, T) -> B) -> B   // given
  fn to_list(self) -> List[T]                      // given
}
```

`verify` proves that a `for` loop over a `FiniteIterator` terminates, as it does for a list. It relies on `len`, which it does not check. Over an iterator that may be unbounded, it reports that the loop may not terminate. `fold` exists only on finite iterators, since on an unbounded one it would never return (`fold` needs a `FiniteIterator`: `N` is an `Iterator` but not a `FiniteIterator`).

Every iterator has `map`, `filter` and `enumerate` (`enumerate(it)` or `it.enumerate()`, pairing each element with its index as `(i, x)`). They are lazy: each returns another iterator that does the work as the loop asks for elements, so they work on unbounded iterators too. A list's `enumerate` is the list builtin and returns a list. A finite iterator stays finite when mapped, filtered or enumerated; a filtered one's `len` counts the elements kept. A function passed to them must terminate, like the code around it. `verify` follows a lambda's calls from the function it is written in (`it calls spin, which may not terminate`), which is how it proves that `fold` over a mapped and filtered finite iterator terminates.

```rust
type Naturals = { at: Int }   // impl Iterator[Int]: 0, 1, 2, … without end

squares := Naturals { at: 0 }.filter(n -> n % 2 == 0).map(n -> n * n)
for s in squares {
  if s > 100 { break }
  print(s)                     // 0, 4, 16, 36, 64, 100
}
Count { at: 0, end: 5 }.map(n -> n + 1).fold(0, (a, b) -> a + b)   // 15
```

On a list, `xs.map(f)`, `filter` and `fold` are the list builtins and return lists.

A comprehension over an iterator is a lazy iterator, so it can be unbounded, and it is finite when its source is. Set and dict comprehensions collect a finite one.

```rust
odd_squares := [n * n for n in Naturals { at: 0 } where n % 2 == 1]   // 1, 9, 25, … on demand
[x * 2 for x in Count { at: 0, end: 4 }].to_list()                    // [0, 2, 4, 6]
Set { x % 2 for x in Count { at: 0, end: 4 } }                        // Set { 0, 1 }
```

Lists, sets and dicts are not iterators (their `map` and `filter` are the list builtins), but `iter()` returns a finite one: the prelude's `ListIter`, over a list's or set's elements or a dict's entries in table order. A range is a list, so `(0..n).iter()` works too.

```rust
fn total[I: FiniteIterator[Int]](it: I) -> Int { it.fold(0, (a, b) -> a + b) }

total([3, 1, 2].iter())                  // 6
total((0..5).iter())                     // 10
for (k, v) in d.iter() { … }             // a dict's entries, in no set order
```

A string is not an iterator either. Its units are always named, and each is a finite iterator:

```rust
s := "héllo 👋🏽"
s.bytes()          // its UTF-8 bytes, 0 to 255: 104, 195, 169, …
s.code_points()    // its code points (Unicode scalar values): 104, 233, …, 128075, 127997
s.graphemes()      // its grapheme clusters (UAX #29), as strings: "h", "é", …, "👋🏽"
s.graphemes().len()                                   // 7
[chr(c) for c in s.code_points() where c > 127]       // lazily: "é", "👋", "🏽"
```

Each walks the string once. `bytes()` and `code_points()` are defined in the prelude and decode the UTF-8 bytes; the only native operations are `byte_len` and `byte_at`. `graphemes()` comes from `std.unicode`, which is loaded only for programs that call it. It implements Unicode 16.0's extended grapheme clusters, including combining marks, emoji sequences, flags, Hangul syllables and Indic conjuncts, and passes every case of Unicode's GraphemeBreakTest.txt.

A finite iterator supports the list syntax for length, indexing and slicing: `len(it)`, `it[i]` (`at`, which requires `i` in range), `get(it, i)`, `it[a..b]`, `it[a..]`. Slicing is lazy (`drop` then `take`), and `join` collects a finite iterator of strings. Every iterator has `take(n)`, which is finite even on an unbounded iterator, and `drop(n)`. Each walks the iterator from the start, so indexing is O(i).

```rust
s := "héllo 👋🏽!"
len(s.graphemes())                  // 8   (len(s.bytes()) is 16)
s.graphemes()[6]                    // "👋🏽"
join(s.graphemes()[1..3], "")       // "él"
Naturals { at: 0 }.drop(3).take(4)  // 3, 4, 5, 6
```

Not supported: nested comprehensions whose inner source is an iterator, and generators.

## Foreign functions: `extern c`

A C function is declared with `extern c fn`: a signature with contracts and no body. A pointer that C hands out is declared with `extern c type`. `examples/fuzzy` uses its own C file and libm this way:

```rust
extern c fn edit_distance(a: String, b: String) -> I32
  ensures it >= 0

/// libm's cube root.
extern c fn cbrt(x: Float) -> Float

/// The fewest single-character edits that turn `a` into `b`.
pub fn distance(a: String, b: String) -> Natural {
  edit_distance(a, b).to_int()
}
```

```toml
# strata.toml: the C the declarations are linked to
[native]
sources = ["c/text.c"]   # the package's own C files
libs = ["m"]             # libraries (-lm)
include_dirs = []        # folders of headers the C files include (-I)
lib_dirs = []            # folders the libraries are in (-L, and searched at run time)
```

Paths are relative to the package's folder, or absolute. `compiler/strata.toml`, for example, links LLVM's C library from Homebrew's `/opt/homebrew/opt/llvm`.

- **Types that cross.** The sized integers (`I8` … `I64`, `U8` … `U64`, C's `int8_t` … `uint64_t`), `Float` (`double`), `Bool`, `String` (copied in each direction as a NUL-terminated string), `()` (`void`, as a result), `extern c type` handles, and `Option` of a handle (a pointer that may be NULL, as `None`). `Int` doesn't cross, since its size isn't fixed. A `mut` parameter is an out-pointer (`mut db: Option[Sqlite3]` is a `sqlite3 **`), copied in and back out after the call, like any `mut`. A `String` can't be `mut`.
- **Handles.** A handle (`extern c type Sqlite3`) is a type with no constructor and no contents. Only C creates one, and Strata code passes it back. Strata doesn't reference-count or free it; C frees it through a function you call. Two handles are equal when they are the same pointer. A C function that returns NULL where a handle is declared is a fault; declare that result `Option[Sqlite3]`.
- **Checks at the boundary.** Arguments are checked against their types (an `I8` is in range) and the `requires` before the call, and the result against its type and the `ensures` after it. A string containing a NUL character can't be passed, since C would read it as the end.
- **The name is the C symbol**, so an extern function can't be renamed. Wrap it instead.
- **Only its own module calls it.** Extern definitions are never `pub`, and other modules, submodules included, can't call them. The declaring module exposes its own functions to the rest of the program, or a capability for anything that touches the outside world, so signatures still show what code can touch.
- **Contracts are assumed.** `verify` never proves foreign code. A foreign function's contract (`ensures it >= 0`) is assumed at each call, and the calling code is verified like any other; fuzzing runs the compiled code, C included. Since nothing proves the contract, compiled programs check it at each call.
- **C calls work everywhere code runs:** in `strata build`, and in `strata run` and `strata test` through the JIT, which compiles the `[native]` sources into a library with clang and loads it. The C standard library (`strlen`, …) needs no `[native]` section.
- **A native module's capability** is requested like any other. `main` and tests name it (`fn main(sqlite: Sqlite)`), its functions receive it as `self` (`sqlite.open(path)`), and the program is linked with the package's `[native]` libraries. std's native libraries are the system's: std.sqlite calls macOS's libsqlite3. `strata build` links it into the executables that use it, against a stub (`compiler/c/libsqlite3.tbd`) so no SDK is needed, and `strata run` and `strata test` load it for the JIT. std's precompiled library excludes the code that calls C, so programs that don't use it don't link it.

Not supported: linking a dependency's `[native]` C into the programs that use it, passing structs by value, callbacks, buffers (so std.sqlite has no blobs), and languages other than C and Python (`extern ts` and `extern rust`, with values crossing as `Json`, are planned; see IDEAS.md).

## Foreign functions: `extern python`

A Python function is declared with `extern python "MODULE" fn`, followed by a signature with contracts and no body. `MODULE` is the Python module that defines it. Values cross as JSON through the derived `Json` trait (std.json), so parameter and result types must have a JSON form: `Int`, `Float`, `Bool`, `String`, `List`, `Option`, `Dict[String, V]`, records and unions that derive `Json`, and aliases of those. `examples/python` calls `text_utils.py`, which sits next to its strata.toml:

```rust
from std.python use PythonError

/// A post summed up (Python builds the `Card` as a dict).
extern python "text_utils" fn summary(post: Post) -> Result[Card, PythonError]

/// Text tools, in Python.
pub capability TextTools {
  fn card(post: Post) -> Result[Card, PythonError] {
    summary(post)
  }
}
```

```toml
# strata.toml (both optional)
[python]
executable = "python3"   # the Python to run (default: python3 on the PATH)
path = ["py"]            # where its modules are, relative to the package (default: the package's folder)
```

- **Every call returns a `Result`.** The result type is written `Result[T, PythonError]` (std.python). These failures are values, never faults:
  - `Raised`: a Python exception, with its type's name, message and traceback. A failed import is reported this way too.
  - `WorkerFailed`: the worker couldn't start, or exited while answering.
  - `BadReply`: the result isn't a `T` (a set, a `NaN`, an integer beyond 2^53, a missing field, a broken refinement).
  - `NotEncodable`: an argument has no JSON form (an integer beyond 2^53).

  `Result[(), PythonError]` accepts any return value.
- **Encoding.** Arguments are encoded with `to_json` and passed positionally; a record becomes a dict with its fields and `"type"`. The return value is decoded with `T.from_json`, checking its refinements. Pydantic models or TypedDicts on the Python side therefore match the derived JSON (see std.json).
- **One worker per run.** The first call starts `python3 -c DRIVER`, with a driver supplied by the runtime. The worker stays up for the rest of the run, imports the modules, and answers each call with one JSON line. Calls from several tasks take turns. Anything the Python code prints goes to standard error. If the worker exits, the call it was answering fails and the next call starts a new worker. Module folders are fixed as absolute paths at compile time, so a built executable finds them wherever it runs.
- **Only the module's capabilities call it.** `extern python` functions are never `pub`, and only capabilities in the same module may call them; the module's other functions and tests can't. Code that reaches Python therefore takes the capability, which `main` and tests ask for.
- **`verify` never runs Python.** An `extern python` function is assumed (`≈`). Its contracts are checked at run time: `requires` before the call and `ensures` after it.

Not supported: Python objects kept on the Python side (handles), keyword arguments, a Python function with a different name from its Strata declaration, a pool of workers, embedding libpython, and `async` functions.

## Exporting functions: `derive C for f`

To let C call Strata, a trait can be derivable over functions (`derivable(Function) trait C`). `derive C for add`, with a function's name after `for` where a type's would go, makes `add` callable from C. `C` is defined in `std.ffi`:

```rust
from std.ffi use C

/// The sum of two numbers.
pub fn add(a: I32, b: I32) -> I32 {
  a + b
}

derive C for add
```

`strata build --lib` builds the program as a shared library (`libNAME.dylib`, or the name given with `-o`). The library exports each derived function under its own name. A header (`NAME.h`) is written next to it, declaring each function after its doc:

```c
/// The sum of two numbers.
int32_t add(int32_t a, int32_t b);
```

```sh
strata build calc.strata --lib                      # libcalc.dylib, calc.h
clang -o use use.c -L. -lcalc -Wl,-rpath,@executable_path
```

- **Types that cross** are those of `extern c`, in the other direction: the sized integers, `Float` (`double`), `Bool` (`bool`), `String` (`const char *` in, copied; `char *` out, a copy the caller frees with `free`), `()` (`void`), handles (`extern c type`, `void *`) and `Option` of a handle (NULL for `None`). A function with any other type, or with type parameters, is rejected at the `derive`, which names the parameter.
- **The library starts its runtime when loaded.** The host keeps its signal handlers and arguments. A library has no `main`; `--lib` leaves it out. A fault in Strata code ends the process, as in a program.
- **A library exports only its derived functions**, plus `strata_string_free` (below). The program's other functions, std's and the runtime's are hidden, so two Strata libraries can be loaded in one process.
- **`derive C for module`** derives `C` for every `pub fn` in the module. A function that can't cross is reported as an error, not skipped.
- **A derive over a function produces top-level definitions**, since a function has no type to attach an `impl` to. Each `derived fn` of the trait is identified by its shape alone:
  - `derived fn export() -> Self = Self` exports the function itself under its own name.
  - A `derived fn` with parameters (`derived fn json(args: String) -> String { .. }`) is a wrapper around the function, exported as `NAME_METHOD` (`add_json`). Its body calls the function with `Self.call(p -> e)` or `Self.try_call(p -> e)`, where `e` gives each argument and `p` is its parameter, a `Field`.
  - `derived fn h() -> String { comptime ... }` produces, at compile time, the function's part of the file `NAME.h`. Here `Self` is the function, as a `std.typing.Type` of kind `Function` with its `name`, `doc` and `parameters`. A file is its parts in order, with each paragraph (up to a blank line) included once.

  `C` is therefore written in Strata, like `Json` (see "Derives on functions" in IDEAS.md).

### `derive Python for f`

`derive Python for add` (`std.ffi`) exports `char *add_json(const char *args)`.

- The arguments arrive as a JSON object keyed by name. Each is decoded with its type's `Json`, refinements checked, and an omitted `Option` is `None`.
- The answer is `{"ok": RESULT}` or `{"error": {"kind", "message", "value"}}`, never a crash. `kind` is `json`, `arguments`, `result`, or `err` for the function's own `Err`, whose value is in `value`.
- The caller frees the answer with `strata_string_free`.

`strata build --lib` writes `NAME.py` next to the library. It is a typed Python module that loads the library with `ctypes`, with one function per derived function, a dataclass per record, a class per union with a subclass per variant, and errors raised as `StrataError`:

```python
import geo                        # geo.py and libgeo.dylib, side by side
geo.middle(geo.Point(0.0, 0.0), geo.Point(2.0, 4.0))   # Point(x=1.0, y=2.0)
geo.divide(1, 0)                  # raises StrataError("err", "ByZero", {"type": "ByZero"})
```

Not supported: `derive TypeScript` and `derive Rust` (the same `NAME_json` export with other files), static libraries, `mut` parameters, and callbacks.

## Programs

**Command-line options** are a record that derives `CommandLine` (`std.cli`). `derive CommandLine for Options` generates the record's `from_args` and `help` at compile time.

- Each field is parsed according to its type:
  - a `Bool` is a flag (`--loud`)
  - an `Option[T]` is an option with a value (`--times 3` or `--times=3`)
  - a `List[T]` takes the remaining positional arguments
  - any other field is a required positional argument, in field order, or an option (`--times 3`) if it has a default
- An omitted field takes its default.
- `T` is an `Int`, `Float` or `String`, a refinement of one, or a choice: a sum type whose variants carry no data (`type Count = | Lines | Words` accepts `lines` or `words`).
- Names use dashes (`dry_run` is `--dry-run`), and `--` ends the options.
- Every refinement is checked (`--times: 0 does not satisfy …`), and a usage error exits with code 2.
- `--help` is generated from the record's and fields' `///` doc comments.
- A field whose doc starts with `-n` can also be given as `-n`.
- An `Option` field whose doc starts with `[name]` is an optional positional argument.
- A `List` field whose doc starts with its flag (`/// --rm …`) or a short name is a repeatable option (`--rm a --rm b`).
- `strata check` rejects a `CommandLine` record that no command line could fill.

**Subcommands** are a union that derives `CommandLine`. Each member is a subcommand (`Edit` is `edit`), its fields are the subcommand's options, and its `///` doc is its help. An options record with a field of that union type parses its other fields as global options before the subcommand.

**`main`'s parameters.** `main` can also take `Console` (`std.io`, chosen by the environment: the terminal) and capabilities, in any order. std provides `Disk` (`std.fs`), `Process` and `Launcher` (`std.process`), `Codebase` (`std.code`: the codebase itself as the tools see it, a `Code`) and `Sqlite` (`std.sqlite`); a program can define its own. `main` passes them on where `Files` or `Environment` is expected: `fn main(mut console: Console, disk: Disk) { wc(mut console, disk) }`, with `wc` taking any `Files` so tests can pass `mem_files(...)`.

**Exit codes.** `main` returns `()` (exit code 0), an `ExitCode` (`Int where 0 <= it && it <= 255`), or a `Result` of either. An `Err` is written to standard error and the exit code is 1. A run-time fault exits with 101. `strata run <fn> [ARGS]` calls a single function instead, with its arguments written as code.

## The standard library

The standard library is the `std/` package. It is written in Strata and built into `strata`, so any program can `use std.…` without declaring a dependency. `strata docgen` generates a reference page for each module from its doc comments ([the index](std/README.md)), listing its types, methods, traits and functions with their signatures, contracts and docs. `strata doc MODULE` shows the same page in the terminal.

| Module | |
|---|---|
| [std.cli](std/cli.md) | Command-line options parsed into a record, and subcommands into a union. |
| [std.code](std/code.md) | A codebase as Strata's tools see it: definitions as text with their dependencies and dependents, checker findings, cached verification results, and the package log. |
| [std.collections](std/collections.md) | Sorted dictionaries and sets (AVL trees), and a priority queue (binary heap). |
| [std.ffi](std/ffi.md) | Calling Strata from other languages: `derive C for f`, `derive Python for f`. |
| [std.fs](std/fs.md) | Files and folders. |
| [std.http](std/http.md) | Requests, responses and routes, for `strata serve`. |
| [std.io](std/io.md) | Console text input and output. |
| [std.json](std/json.md) | JSON text, and reading and writing records and sum types as JSON. |
| [std.memory](std/memory.md) | Arenas, for programs that must be fast. |
| [std.process](std/process.md) | The running program's arguments, environment variables and exit code, and running other programs. |
| [std.orm](std/orm.md) | Records stored as SQLite tables: `derive Table` (the schema, rows written and read back with checks) and typed queries over their fields. |
| [std.regex](std/regex.md) | Regular expressions. |
| [std.sqlite](std/sqlite.md) | SQLite databases through the system's libsqlite3: connections, prepared statements and transactions. |
| [std.term](std/term.md) | Terminal text styles: bold, dim, colors. |
| [std.time](std/time.md) | The clock: the current time, sleeping, and timers in `select`. |
| [std.toml](std/toml.md) | TOML 1.1 text, and reading and writing records and sum types as TOML. |
| [std.typing](std/typing.md) | Types as values, for derivable traits. |
| [std.unicode](std/unicode.md) | Code point sets, Unicode properties and simple case folding (Unicode 16.0.0). |
| [std.unicode_tables](std/unicode_tables.md) | Unicode 16.0.0 character data, for `std.unicode`. |
| [std.utf8](std/utf8.md) | Encoding text as UTF-8 bytes and decoding it. |
| [std.web](std/web.md) | HTML pages built as values. |

**JSON** (`std.json`) is read into records and sum types that derive `Json`, in the form a web front end sends:

- A record is an object. Writing adds `"type"`, the type's name; when reading, `"type"` is optional.
- An omitted field takes its default.
- An `Option` is the value, or `null`, or omitted.
- A sum's variant is an object whose `"type"` names it (`{"type": "Circle", "r": 2}`). A single positional field is written as `"value"`.
- A sum whose variants carry no data is a string (`"Quiet"`).
- A tuple is an array, and a `distinct` type is its underlying value.
- A union is its member's value. Reading takes the first member the JSON fits. Unions whose members' JSON forms overlap are rejected: `Int | Float`, two list types, `String` next to a choice, or two members with the same `"type"`.

Every refinement is checked, and errors give the path: `home.zip: "1234" does not satisfy …`. Keys that the record doesn't have are ignored, unless `strata.toml` sets `unknown_keys = "error"` under `[json]`. `from_json_with(j, Reject)` or `from_json_with(j, Ignore)` decides for a single call.

Integers must be within ±(2^53 − 1), the integers a JavaScript number holds exactly (`std.json.SafeInt`). Reading or writing a larger one is an error, which is why `to_json` returns a `Result`. A package that wants this proven instead sets `integers = "proven"` under `[json]`. Every integer field must then be shown to be a `SafeInt`, by its type or a refinement within it, and `strata verify` fails on any field that might not be.

A type with no JSON form (a function, `Option[Option[T]]`, a field named `type`, a variant with several positional fields) is an error at the `derive`, and so is a field whose type doesn't derive `Json`. A generic type derives `Json` for every type argument that has it (`derive Json for Pair` covers `Pair[T]` for each `T: Json`). Through the generic impls, `Some(x)` where `x` is written as `null` (`Some(None)`) is an error from `to_json`, and `from_json` checks the elements' refinements (`read[List[Port]]("[70000]")` is an `Err` naming `Port`).

```rust
from std.json use Json, parse, stringify

derive Json for
type Signup = { email: String where contains(it, "@"), age: Int where it >= 13, nickname: Option[String] }

signup := Signup.from_json(parse(body)!)!
text := stringify(signup.to_json()!)  // {"type":"Signup","email":…,"age":…,"nickname":null}
```

**TOML** (`std.toml`) has a parser written in Strata, which the tools also use to read `strata.toml`. It enforces every rule of TOML 1.1:

- keys and tables defined twice
- tables closed once written: inline tables, and tables created by dotted keys can't get a `[header]`, nor can header tables be extended by dotted keys
- integers within 64 bits
- dates that exist (`2023-02-29` is an error)
- optional seconds (`07:32`)
- inline tables over several lines with a trailing comma
- `\e` and `\xHH` escapes

A Strata `Float` is finite, so `inf` and `nan` are errors. The case `toml-documents` checks the parser's readings of 104 documents (`tests/data/toml-documents.out`), and that what `stringify` writes reads back as the same value.

```rust
from std.toml use parse, lookup, Integer

doc := parse("[server]\nport = 8080\n")!
port := match lookup(doc, ["server", "port"]) {
  Some(Integer(n)) => n,
  _ => 80,
}
```

`derive Toml` reads records and sum types from TOML as `derive Json` does from JSON, in the form of a configuration file:

- A record is a table of its fields, with no `type` key.
- An omitted field takes its default, and an `Option` field is omitted for `None`. TOML has no null, so an `Option` anywhere other than a field is an error at the `derive`.
- A sum's variant is a table whose `type` names it, with a single positional field written as `value`. A sum whose variants carry no data is a string.
- A tuple is an array, a `Dict[String, V]` is a table, and `Date`, `Time` and `DateTime` are TOML's dates and times.
- Every refinement is checked, and errors give the path (`servers[0].port: 70000 does not satisfy …`).
- Integers are 64-bit.
- Unknown keys are ignored unless `strata.toml` sets `unknown_keys = "error"` under `[toml]`, or the call uses `from_toml_with(v, Reject)`.

The methods are named for TOML (`to_toml`, `from_toml`, `read_toml`, `write_toml`), so a type can derive both `Toml` and `Json`.

```rust
from std.toml use Toml

type Server = { host: String, port: Int where 0 < it && it < 65536 = 8080 }
type Config = { name: String, servers: List[Server] = [] }
derive Toml for Server
derive Toml for Config

config := Config.read_toml("name = \"app\"\n[[servers]]\nhost = \"a.example\"\n")!
text := config.write_toml()!
```

**Tables** (`std.orm`). A record that derives `Table` is stored as the rows of an SQLite table (`std.sqlite`), named after the type in snake case (`TodoItem` becomes `todo_item`).

- **Columns.** Each field is a column. `Int` and `Bool` are `INTEGER`, `Float` is `REAL`, and `String` is `TEXT`. An `Option` is the same column without `NOT NULL`, with `None` as NULL. A `distinct` type is stored as its underlying type. A record, union, list or dict whose type derives `Json` is a JSONB column: written with `jsonb(?)`, read with `json(..)`, decoded and checked against its refinements (a row it refuses is `BadRow`). A record field whose type neither is a `Key[T]` nor derives `Json`, any other field type, and `Option[Option[T]]`, is an error at the `derive`.
- **Keys.** The key is written in the derive's block: `derive Table for Item { key code }`, or `key (shelf, slot)` for a key of several columns (a table `PRIMARY KEY`). Without a `key`, the key is the field `id`; a table with neither is an error. The default `id` key, when it is an integer, is SQLite's row id: `insert` assigns one to each new row, ignoring the `id` given (`TodoId(0)` by default), and returns the record with the new id. Any other key is inserted as given.
- **References.** A field of type `Key[T]` (`project: Key[Project]`, where `Project` derives `Table` with a key of one integer or text column) is a foreign key: its column is `T`'s key column with `REFERENCES "project" ("id")`, which `create` has SQLite enforce. `Option[Key[T]]` may be NULL. `p.key()` is a row's `Key`; `k.load(db)` reads the row it names, only when asked.
- **Invariants.** Both keys are invariants that `verify` checks over the database's model (`Database.rows`, each row's table and key): `insert` requires each row its `Key` fields name to exist and its key to be new (unless SQLite gives it), `update` requires the first, and each says how the rows change. An insert naming a row that may not exist is reported by `verify` with a counterexample. At run time SQLite refuses it, and refuses to delete a row another names, both as `Constraint` errors.
- **Checks.** A refinement becomes a `CHECK` where SQL can express it: comparisons, `+`, `-`, `*`, `byte_len(it)`, numbers, strings, `true`, `false`, `!`, `&&` and `||`. For a refinement joined by `&&`, the parts SQL can express become checks (`it != "" && !contains(it, "/")` checks `<> ''`). A record's own refinement (`where it.start <= it.end`) becomes a table `CHECK`. A literal default becomes the column's `DEFAULT`.
- **Reading back.** Every row read is checked against the record's refinements again. A row that SQL accepted but that breaks one is an error value, `BadRow { table, message }`, never a fault.
- Every operation returns a `Result[_, SqlError]` and runs on a `Database`, so tests can pass an in-memory database or a `mock`.

```rust
from std.orm use Table, Compare, Rows

type Todo = {
  id: TodoId = TodoId(0),
  title: String where it != "" && byte_len(it) <= 80,
  priority: Int where 1 <= it && it <= 5,
  done: Bool = false,
}
derive Table for Todo

Todo.schema().create(mut db)!                                // CREATE TABLE IF NOT EXISTS
t := (Todo { title: "write docs", priority: 2 }).insert(mut db)!
_ := (Todo { ..t, done: true }).update(mut db)!             // by key: whether it was there
found := Todo.get(db, t.id)!                                  // Option[Todo]
open := Todo.filter(Todo.done.eq(false).and(Todo.priority.le(3)))
  .order_by(Todo.priority)
  .limit(10)
  .all(db)!
```

**Queries** are values: a `Query[Todo]`, whose condition is a `Cond[Todo]`.

- Start with `Todo.query()` (every row) or `Todo.filter(c)`, refine with `filter`, `order_by`, `order_by_desc` and `limit`, and run with `all`, `first`, `count` or `delete`.
- A condition compares a column, named by its key path, with a value of the field's type: `eq` and `ne` (SQL's `IS` and `IS NOT`, so `Todo.due.eq(None)` matches NULL), and `lt`, `le`, `gt` and `ge` (an ordering against `None` matches no row). Conditions combine with `and`, `either` (`or` is a keyword) and `not`.
- The checker sees the types through, so a value of another type, a column of another table, or a field the record doesn't have is an error.
- Values are always passed as parameters (`"done" IS ?`), never as SQL text.
- A `distinct` key compared in a query needs `impl Sql for TodoId` (returning `self.value`).
- `Todo.schema()` describes the table (its name, columns and `sql()`), `t.values()` gives a record's values, and `Todo.from_row(r)` turns a row back into a record.

See `examples/todos_db.strata`. Not supported: joins (a project's to-dos as a query), referencing a key of several columns, migrations (a changed record against an existing table), transactions as a scope, `LIKE` and `IN`, blobs, and reading a record's fields by column name.

**Regular expressions** (`std.regex`) are implemented in Strata. `parse` reads the syntax of Rust's `regex` crate, with Unicode by default:

- `\d`, `\w`, `\s` and `.` are Unicode-aware
- Unicode properties: `\p{Greek}`, `\p{gc=Lu}`, `\p{age=6.0}`
- class operations `&&`, `--` and `~~`, and `[[:alpha:]]`
- named groups, the flags `imsUuRx`, and the assertions `\b`, `\b{start}`, `\A`, `\z`
- no look-around or back-references

It produces the same tree as the regex-syntax crate: classes as ranges of code points with `(?i)` folded in, flags removed, and the same simplifications (`ab|ac` is `a(?:b|c)`, `a|b` is `[ab]`). The case `regex-curated` checks the trees of curated patterns (`tests/data/regex-curated.out`). The Unicode data (`std.unicode_tables`) is generated by `tools/unicode_tables.py` from regex-syntax's tables (Unicode 16.0.0).

```rust
from std.regex use parse, show

re := parse("(?i)ab|ac")!
show(re)    // cat(cls(41-41 61-61),cls(42-43 62-63))
```

**Capabilities in std.** Each capability is a trait, so code that prints takes a `Console`, and its signature says so. The real implementations are capabilities the runtime creates, for `main` and for tests that name one; nothing else can create them. Other tests pass in-memory implementations (see `examples/wc`) or `mock Console`. Only `std` can call the native `host_*` operations underneath, and `verify` and fuzzing never run code that reaches them.
