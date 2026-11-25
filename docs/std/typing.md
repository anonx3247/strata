# std.typing

Types as values at compile time, for derivable traits.
A `derivable(Record) trait` sees the type it is derived for as `Self`, and each `derived fn` is specialized for it.

## Types

### Type

```rust
type Type = {
  /// What it is called: `Option`, `Port`, a member's name. A refined type has its base's name.
  /// `None` for `()`, a union written out, or a function type.
  name: Option[String],
  /// Its doc comment, line by line.
  doc: List[String],
  kind: Kind,
  /// Type arguments: `[String]` for `Option[String]`.
  args: List[Type],
  /// As written (`List[(Int, Int)]`), for messages.
  written: String,
  /// Its refinement as written (`it > 0`), if any.
  condition: Option[String],
  /// The fields of a record, or of a union member with named fields.
  fields: List[Field],
  /// The elements of a tuple (`(Int, String)`) or a tuple-shaped member (`Circle(Float)`).
  elements: List[Type],
  /// A union's members in order. A union among them gives its own members.
  /// `type Option[T] = Some(T) | None` has `Some(T)` and `None`.
  members: List[Type],
  /// A function type's parameters and result.
  params: List[Type],
  result: Option[Type],
  /// A function's named parameters, for `derive C for add` where `Self` is the function `add`.
  parameters: List[Field] = [],
  /// What an alias or a `distinct` type stands for: `Int where it > 0` for `type Count = Int where it > 0`.
  /// Its `name` is its own.
  base: Option[Type] = None,
  /// The clauses of the derive block it is derived with, on `Self` in a
  /// `derived fn` (`derive Table for Todo { key code }`: `key` and
  /// `["code"]`); empty elsewhere.
  clauses: List[Clause] = [],
}
```

A type, known while compiling: `Int`, `Option[String]`, a record, a union
or one of its members, `Int where it > 0`, `(Int) -> Bool`.
Only the lists of its kind are filled. The others are empty.
In a `derived fn`, where `Self` is the type derived for:

- `comptime e` is evaluated while compiling and its value written in its

place.

- `comptime for f in Self.fields { .. }` writes the body once per field.
- `match self { m => e for m in Self.members }` writes one arm per member.

In the arm, `self` is that member, `f.of(self)` reads its field `f`, and
`self.0` its first element.

- `match comptime Self { Record => .., Union => .. }` and `match comptime

f.type { Int => .., Option[x] => .. }` keep the one arm that fits. `x` is
then a type too. `if comptime c { .. } else { .. }` keeps the branch
taken.

- `Self.make(f -> e)` builds the record, `e` giving each field's value.

`Self.try_make(f -> e)` takes a `Result` per field and returns the first
`Err`, else `Ok` of the value.

- `m.make(..)` and `m.try_make(..)` build the member `m` known while

compiling. `m.make()` builds an atom, `m.make((i, e) -> x)` a tuple-shaped
member from each element's index and type.

- `T.m(..)` calls the method `m` of a type known while compiling: `x` in

`Option[x] =>`, `t` for a `comptime t: Type`, `f.type`, `Self`.

- `t.check(x)` is `Ok(())` when `x` meets the refinements of `t`, else

`Err` with what it breaks.

- For a `distinct` type `t`, `match comptime t { Distinct[x] => .. }`

binds what it is made of, and `t(v)` makes one.

- A function with a `comptime` parameter (`fn read(comptime f: Field,

..)`) is compiled once per value given, so `f.type` is a real type there.

### Kind

```rust
type Kind =
  // has `fields`
  | Record
  // has `elements`
  | Tuple
  // has `members`
  | Union
  // has `params` and `result`
  | Function
  // one value: a type without fields (`Quiet`, `None`)
  | Atom
  // one value: `()`
  | Unit
  // `Int`, `Float`, `Bool`, `String`, `List[x]`, `Set[x]`, `Dict[k, v]`, told apart by name and `args`
  | Builtin
  // a type parameter (`T` of `Pair[T]`)
  | Param
```

What a type's values are made of. A trait is `derivable` for a kind, a `Choice`, or a union of them (`Record | Union`).

### Field

```rust
type Field = {
  name: String,
  /// Its doc comment, line by line.
  doc: List[String],
  type: Type,
  /// Its default value as written: `Some("1")` for `times: Int = 1`.
  default: Option[String],
}
```

A field of a record, or a parameter of a function. A parameter's `default` is `None`.

### Choice

```rust
type Choice = Type
  where it.kind is Union && all(it.members, m -> m.kind is Atom)
```

A union whose members are all atoms: `type Level = Quiet | Loud`.

### Clause

```rust
type Clause = { name: String, names: List[String] }
```

A clause of a derive block: its word and the names after it (`key
(project, slug)`: `key` and `["project", "slug"]`). What it means is
the trait's.

## Functions

### compile_error

```rust
fn compile_error[T](comptime message: String) -> T
```

Stops the `derive` with `message` as an error at the `derive`.
Written in an arm of `match comptime` that only some types reach.

### setting

```rust
fn setting(comptime key: String) -> Option[String]
```

The package's `strata.toml` setting, known while compiling: `setting("json.unknown_keys")` is `[json]`'s `unknown_keys`.

### implements

```rust
fn implements(comptime t: Type, comptime trait_name: String) -> Bool
```

Whether the type `t` has an `impl` (or a `derive`) of the trait named
`trait_name` (`"Json"`, or qualified): known while compiling, so `if
comptime implements(t, "Json") { .. }` keeps the branch that fits.
