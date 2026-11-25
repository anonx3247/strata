# std.toml

TOML 1.1 text (https://toml.io/en/v1.1.0) read into values and written from them, and `derive Toml` for records and sum types.
`parse` checks every rule of TOML 1.1. Not supported: `inf` and `nan`, since a Strata `Float` is finite.

## Types

### TomlValue

```rust
type TomlValue =
  | Text(String)
  | Integer(Int)
  | Number(Float)
  | Boolean(Bool)
  | Moment(DateTime)
  | Array(List[TomlValue])
  | Table(List[(String, TomlValue)])
```

A TOML value.

### Date

```rust
type Date = { year: Int, month: Int, day: Int }
```

A date: `1979-05-27`.

### Time

```rust
type Time = { hour: Int, minute: Int, second: Int, nanosecond: Int }
```

A time of day: `07:32:00.999`. Seconds may be left out: `07:32`.

### Offset

```rust
type Offset =
  | Utc
  | Minutes(Int)
```

A time's offset from UTC: `Z`, or `+01:30` (90 minutes).

### DateTime

```rust
type DateTime = {
  date: Option[Date],
  time: Option[Time],
  offset: Option[Offset],
}
```

TOML's four kinds of date and time: an offset date-time has all three parts, a local date-time no offset, a local date only the date, a local time only the time.

## Traits

### Toml

```rust
derivable(Record | Union) trait Toml {
  /// The value as TOML.
  fn to_toml(self) -> Result[TomlValue, String]
  /// The value as a TOML document. Only a table, such as a record's, is one.
  fn write_toml(self) -> Result[String, String]
  /// `path` says where the value is, for messages. `""` at the top.
  derived fn to_toml_at(self, path: String) -> Result[TomlValue, String]
  /// Every refinement checked. `path` says where the value is, for messages. `""` at the top.
  derived fn from_toml_at(
    v: TomlValue,
    path: String,
    keys: UnknownKeys,
  ) -> Result[Self, String]
  /// A value read from TOML, every refinement checked.
  fn from_toml(v: TomlValue) -> Result[Self, String]
  /// `from_toml`, with what to do with keys a record doesn't have.
  fn from_toml_with(v: TomlValue, keys: UnknownKeys) -> Result[Self, String]
  /// A value read from a TOML document: `Config.read_toml(text)`.
  fn read_toml(text: String) -> Result[Self, String]
}
```

A record or a sum type read from TOML and written to it. The TOML is what a person writes in a configuration file:

- A record is a table of its fields. A field left out takes its default. An `Option` field is left out for `None`, since TOML has no null, so `Option` is only for fields.
- A union member that is a record, an atom or tuple-shaped is a table whose `type` names it, with its fields: `{ type = "Circle", r = 2 }`. One positional field is `value`.
- An atom may also be the bare string `"Dot"`. A union whose members are all atoms is just the string.
- Any other member is read as the first one the TOML fits.
- `Int` is a 64-bit integer, and writing a larger one is an error. `Float` is a float, or an integer when reading.
- `List[T]` and tuples are arrays. `Dict[String, V]` is a table. A `distinct` type is its underlying value. `Date`, `Time` and `DateTime` are TOML's dates and times.
- Keys a record doesn't have are ignored, unless `unknown_keys = "error"` is set under `[toml]`. `from_toml_with` chooses for one call.
- Members whose TOML overlaps (`Int | Float`, two lists, two members named alike) are an error at the `derive`.

The methods are named for TOML, so a type may derive `Json` too.

## Functions

### lookup

```rust
fn lookup(
  table: List[(String, TomlValue)],
  path: List[String],
) -> Option[TomlValue]
```

The value at a path of keys in a table: `lookup(doc, ["server", "port"])`.

### parse

```rust
fn parse(text: String) -> Result[List[(String, TomlValue)], String]
```

Parses TOML text into its root table, keys in the order written.
A document that breaks a rule is an `Err` saying where (`line:column: ...`): keys and tables defined twice, tables extended once closed, integers beyond 64 bits, dates that don't exist (`2023-02-29`), control characters.

### stringify

```rust
fn stringify(table: List[(String, TomlValue)]) -> String
```

A table as a TOML document.
Its values come first, then a `[header]` section for each table in it, and a `[[header]]` for each table of each array of tables.
A table inside an array of other values is written inline.

### quote

```rust
fn quote(s: String) -> String
```

A string as a TOML basic string.

## Implementations

- `impl Toml for Int`
- `impl Toml for Float`
- `impl Toml for Bool`
- `impl Toml for String`
- `impl Toml for DateTime`
- `impl Toml for Date`
- `impl Toml for Time`
- `impl[T: Toml] Toml for List[T]`
- `impl[V: Toml] Toml for Dict[String, V]`
