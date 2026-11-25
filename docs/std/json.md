# std.json

std.json: JSON text, and records and sum types read from it and written to it.
`derive Json for T` writes the JSON a web front end sends and expects.

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

An integer a JavaScript number holds exactly: ±(2^53 - 1). JSON integers are limited to it, since a JavaScript reader would round a larger one.

## Traits

### Json

```rust
derivable(Record | Union) trait Json {
  /// The value as JSON.
  fn to_json(self) -> Result[JsonValue, String]
  /// `path` says where the value is, for messages. `""` at the top.
  derived fn to_json_at(self, path: String) -> Result[JsonValue, String]
  /// Every refinement checked. `path` says where the value is, for messages. `""` at the top.
  derived fn from_json_at(
    j: JsonValue,
    path: String,
    keys: UnknownKeys,
  ) -> Result[Self, String]
  /// A value read from JSON, every refinement checked.
  fn from_json(j: JsonValue) -> Result[Self, String]
  /// `from_json`, with what to do with keys a record doesn't have.
  fn from_json_with(j: JsonValue, keys: UnknownKeys) -> Result[Self, String]
}
```

A record or a sum type read from JSON and written to it. The JSON:

- A record is an object with its fields and `"type"`, its type's name. Reading, `"type"` may be left out, a field left out takes its default, and an `Option` field left out or `null` is `None`.
- `Option[T]` is the value or `null`.
- A union member that is a record, an atom or tuple-shaped is an object whose `"type"` names it, with its fields: `{"type": "Circle", "r": 2}`. One positional field is `"value"`: `{"type": "Pixels", "value": 3}`.
- An atom may also be the bare string `"Dot"`. A union whose members are all atoms is just the string: `"Quiet"`.
- Any other member is read as the first one the JSON fits.
- `Int` is a number within ±(2^53 - 1) (`SafeInt`). A larger one is an error. With `integers = "proven"` under `[json]`, `strata verify` must prove each integer field is a `SafeInt` instead.
- `Float` is a number, an integer included. `List[T]` and tuples are arrays. `Dict[String, V]` is an object. A `distinct` type is its underlying value.
- Keys a record doesn't have are ignored, unless `unknown_keys = "error"` is set under `[json]`. `from_json_with` chooses for one call.

Errors at the `derive`:

- union members whose JSON overlaps: `Int | Float`, two lists, two members named alike
- a type with no JSON form: a function, `Option[Option[T]]`, a field named `type`, a member with several positional fields

A field whose type is another record or named union needs `Json` derived for it too.

## Functions

### encode_as

```rust
fn encode_as(
  comptime t: Type,
  x: t,
  step: String,
  outer: String,
) -> Result[JsonValue, String]
```

A value of type `t` as JSON. Used by `derive Json`, and by `derive Python` (std.ffi) for answers.
`step` after `outer` says where it is, for messages.

### decode_as

```rust
fn decode_as(
  comptime t: Type,
  j: JsonValue,
  path: String,
  keys: UnknownKeys,
) -> Result[t, String]
```

A value of type `t` read from JSON, refinements checked. Used by `derive Json`, and by `derive Python` (std.ffi) for arguments.
`path` says where it is, for messages.

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

Parses JSON text.

## Implementations

- `impl Json for Int`
- `impl Json for Float`
- `impl Json for Bool`
- `impl Json for String`
- `impl[T: Json] Json for List[T]`
- `impl[T: Json] Json for Option[T]`
- `impl[V: Json] Json for Dict[String, V]`
