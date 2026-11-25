# std.json

JSON text, and records and sum types read from it and written to it.

Derive `Json` for a record or a sum type:

```rust
type Signup = {
  email: String where contains(it, "@"),
  age: Int where it >= 13,
  nickname: Option[String],
  tags: List[String] = [],
}

derive Json for Signup

from std.json use Json, parse, stringify
signup := Signup.from_json(parse(body)!)!    // checks every refinement
text := stringify(signup.to_json()!)
```

The JSON is the one a web front end sends and expects:

- a record is an object with its fields, and `"type"`: its type's name.
  Reading, `"type"` may be left out; a field left out takes its default,
  and an `Option` field left out (or `null`) is `None`;
- `Option[T]`: the value, or `null`;
- a union: its member's value. A member that is a record, an atom or
  tuple-shaped is an object whose `"type"` names it, with its fields
  (`{"type": "Circle", "r": 2}`); one positional field is `"value"`
  (`{"type": "Pixels", "value": 3}`). An atom may also be the bare
  string (`"Dot"`), and a union whose members all carry nothing
  (`type Level = Quiet | Loud`) is just the string: `"Quiet"`. Reading,
  any other member is the first the JSON fits;
- `Int` is a number within ±(2^53 - 1) (`SafeInt`), the integers a
  JavaScript number holds exactly: reading or writing a larger one is
  an error (`to_json` returns a `Result`). With `integers = "proven"`
  under `[json]`, every integer field must be shown to be a `SafeInt`
  instead (`strata verify` fails on one that may not be, and `to_json`
  never meets one); `Float` a number (an integer too);
  `List[T]` an array; a tuple an array of its values; `Dict[String, V]`
  an object; a `distinct` type its underlying value;
- members whose JSON overlaps (`Int | Float`, two lists, two members
  named alike) are an error at the `derive`.

Keys a record doesn't have are ignored, unless the package says
`unknown_keys = "error"` under `[json]` in its `strata.toml`;
`from_json_with(j, Reject)` / `from_json_with(j, Ignore)` choose for one call.
A type that has no such form (a function, `Option[Option[T]]`, a field
named `type`, a member with several positional fields) is an error at
the `derive`. A field whose type is another record or named union needs
`Json` derived for it too.

## Types

### JsonValue

```rust
type JsonValue =
  | Null
  | Boolean(Bool)
  | Integer(Int)
  | Number(Float)
  | Text(String)
  | Array(List[JsonValue])
  | Object(List[(String, JsonValue)])
```

A JSON value.

### UnknownKeys

```rust
type UnknownKeys =
  | Ignore
  | Reject
```

What to do with a key a record doesn't have.

### SafeInt

```rust
type SafeInt = Int
  where -9007199254740991 <= it && it <= 9007199254740991
```

An integer a JavaScript number holds exactly, ±(2^53 - 1): what JSON's
integers are here. Beyond it, a JavaScript reader would silently round.

## Traits

### Json

```rust
derivable(Record | Union) trait Json {
  /// The value as JSON.
  fn to_json(self) -> Result[JsonValue, String]
  /// The value as JSON; `path` says where it is, for messages (`""` at the
  /// top).
  derived fn to_json_at(self, path: String) -> Result[JsonValue, String]
  /// A value read from JSON, every refinement checked; `path` says where
  /// it is, for messages (`""` at the top).
  derived fn from_json_at(
    j: JsonValue,
    path: String,
    keys: UnknownKeys,
  ) -> Result[Self, String]
  /// A value read from JSON, every refinement checked.
  fn from_json(j: JsonValue) -> Result[Self, String]
  /// `from_json`, with what to do with keys a record doesn't have given.
  fn from_json_with(j: JsonValue, keys: UnknownKeys) -> Result[Self, String]
}
```

A record or a sum type read from JSON and written to it (see above).

## Functions

### stringify

```rust
fn stringify(j: JsonValue) -> String
```

A JSON value as compact text.

### quote

```rust
fn quote(s: String) -> String
```

A string as a JSON string literal.

### parse

```rust
fn parse(text: String) -> Result[JsonValue, String]
```

JSON text read as a value.

## Implementations

- `impl Json for Int`
- `impl Json for Float`
- `impl Json for Bool`
- `impl Json for String`
- `impl[T: Json] Json for List[T]`
- `impl[T: Json] Json for Option[T]`
- `impl[V: Json] Json for Dict[String, V]`
