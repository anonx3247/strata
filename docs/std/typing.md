# std.typing

Types as values, for derivable traits.

While a program is compiled, a type is a value: a `Type`. A derivable trait
names the kinds of type it can be derived for, and its `derived fn`s see
the type derived for as `Self`:

```rust
pub derivable(Record) trait Named {
  derived fn names() -> List[String] {
    comptime Self.fields.map(f -> f.name)
  }
}

derive Named for Point
```

The kind is a `Kind` (`Record`, `Union`, ...), `Choice` (a union whose
members are all atoms), or a union of them (`derivable(Record | Union)`).
When a trait is derived for a type, each `derived fn` is specialized for
it while the program is compiled, and `comptime` code runs then:

- `comptime e` is evaluated, its value written in its place;
- `comptime for f in Self.fields { .. }` writes the body once per field;
- `match self { m => e for m in Self.members }` writes one arm per
  member; in the arm, `self` is that member, `f.of(self)` reads its
  field `f`, and `self.0` its first element (a tuple-shaped one);
- `match comptime Self { Record => .., Union => .. }` and
  `match comptime f.type { Int => .., Option[x] => .. }` keep the one arm
  that fits (`x` is then a type too); `if comptime c { .. } else { .. }`
  the branch taken;
- `Self.make(f -> e)` builds the record, `e` giving each field's value
  (`f` is that field); `Self.try_make(f -> e)` the same with a `Result`
  for each field: the first `Err` is the result, else `Ok` of the new
  value; `m.make(..)` / `m.try_make(..)` build the member `m`, known
  while compiling (in a `comptime for` or an arm): `m.make()` an atom,
  `m.make((i, e) -> x)` a tuple-shaped one, from each element's index
  and type;
- `T.m(..)` calls the method `m` of a type known while compiling (`x`
  in `Option[x] =>`, `t` for a `comptime t: Type`, `f.type`, `Self`);
- `t.check(x)` is `Ok(())` when `x` meets the refinements of `t`, else
  `Err` with what it breaks;
- `compile_error(message)`, reached, stops the `derive` with the message;
- `setting("section.key")` is the package's setting (`strata.toml`);
- for a `distinct` type `t`, `match comptime t { Distinct[x] => .. }`
  binds what it is made of, and `t(v)` makes one; a match comprehension
  over a union's members (`match v { m => .. for m in t.members }`) has
  an arm per member type.

A function with a `comptime` parameter (`fn read(comptime f: Field, ..)`)
is compiled once per value it is given, so `f.type` is a real type there.

## Types

### Type

```rust
type Type = {
  /// What it is called (`Option`, `Port`, a member's name; a refined type
  /// is called as its base): `None` for `()`, a union or a function type
  /// written out.
  name: Option[String],
  /// Its doc comment, line by line.
  doc: List[String],
  kind: Kind,
  /// The type arguments it is given (`Option[String]`: `[String]`).
  args: List[Type],
  /// As written (`List[(Int, Int)]`), for messages.
  written: String,
  /// Its refinement, as written (`it > 0`), if it has one.
  condition: Option[String],
  /// A record's fields (a record type, or a union's member with named fields).
  fields: List[Field],
  /// A tuple's elements (`(Int, String)`, or a member `Circle(Float)`).
  elements: List[Type],
  /// A union's members, in order, a union among them giving its own
  /// (`type Option[T] = Some(T) | None`: `Some(T)`, `None`).
  members: List[Type],
  /// A function type's parameters and result.
  params: List[Type],
  result: Option[Type],
  /// A function's parameters, by name, when it is a function rather than a
  /// function type (`derive C for add`: `Self` is `add`, its `name` and
  /// `doc` its own).
  parameters: List[Field] = [],
  /// What an alias or a `distinct` type stands for (`type Count = Int where
  /// it > 0`: `Int where it > 0`), when it is one: its `name` is its own.
  base: Option[Type] = None,
  /// The clauses of the derive block it is derived with, on `Self` in a
  /// `derived fn` (`derive Table for Todo { key code }`: `key` and
  /// `["code"]`); empty elsewhere.
  clauses: List[Clause] = [],
}
```

A type, known while compiling: `Int`, `Option[String]`, a record, a
union or one of its members, `Int where it > 0`, `(Int) -> Bool`.
Which lists it has depends on its kind; the others are empty.

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
  // `Int`, `Float`, `Bool`, `String`, `List[x]`, `Set[x]`, `Dict[k, v]`:
  // told apart by name and `args`
  | Builtin
  // a type parameter (`T` of `Pair[T]`)
  | Param
```

The shape of a type: what its values are made of.

### Field

```rust
type Field = {
  name: String,
  /// Its doc comment, line by line.
  doc: List[String],
  type: Type,
  /// Its default value, as written (`times: Int = 1` has `Some("1")`).
  default: Option[String],
}
```

A field of a record, or a parameter of a function (`default` is then `None`).

### Choice

```rust
type Choice = Type
  where it.kind is Union && all(it.members, m -> m.kind is Atom)
```

A union whose members carry nothing (`type Level = Quiet | Loud`).

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

Stop deriving with this message, as an error at the `derive`: written
in an arm of `match comptime` that only some types reach.

### setting

```rust
fn setting(comptime key: String) -> Option[String]
```

A setting of the package being compiled, from its `strata.toml`
(`setting("json.unknown_keys")` is `[json]`'s `unknown_keys`), if set:
known while compiling.

### implements

```rust
fn implements(comptime t: Type, comptime trait_name: String) -> Bool
```

Whether the type `t` has an `impl` (or a `derive`) of the trait named
`trait_name` (`"Json"`, or qualified): known while compiling, so `if
comptime implements(t, "Json") { .. }` keeps the branch that fits.
