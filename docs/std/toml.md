# std.toml

TOML text (version 1.1, https://toml.io/en/v1.1.0) read into
values and written from them, and records and sum types read from TOML
and written to it.

Derive `Toml` for a record or a sum type:

```rust
type Config = {
  name: String where it != "",
  port: Int where 0 < it && it < 65536 = 8080,
  owner: Option[String],
  tags: List[String] = [],
}

derive Toml for Config

from std.toml use Toml
config := Config.read_toml(disk.read("app.toml")!)!  // checks every refinement
text := config.write_toml()!
```

The TOML is what a person writes in a configuration file:

- a record is a table of its fields. A field left out takes its
  default, and an `Option` field is left out for `None` (TOML has no
  null, so `Option` is only for fields);
- a union: its member's value. A member that is a record, an atom or
  tuple-shaped is a table whose `type` names it, with its fields
  (`{ type = "Circle", r = 2 }`); one positional field is `value`. An
  atom may also be the bare string (`"Dot"`), and a union whose members
  all carry nothing (`type Level = Quiet | Loud`) is just the string.
  Reading, any other member is the first the TOML fits;
- `Int` an integer (64 bits: writing a larger one is an error), `Float`
  a float (an integer too, reading), `Bool`, `String`, `List[T]` an
  array, a tuple an array of its values, `Dict[String, V]` a table, a
  `distinct` type its underlying value, `Date`, `Time` and `DateTime`
  TOML's dates and times;
- members whose TOML overlaps (`Int | Float`, two lists, two members
  named alike) are an error at the `derive`.

Keys a record doesn't have are ignored, unless the package says
`unknown_keys = "error"` under `[toml]` in its `strata.toml`;
`from_toml_with(v, Reject)` / `from_toml_with(v, Ignore)` choose for one
call. The methods are named for TOML (`to_toml`, `from_toml`), so a type
may derive `Json` too.

Reading TOML text into values:

```rust
from std.toml use parse, lookup, Text
doc := parse(disk.read("strata.toml")!)!
name := match lookup(doc, ["package", "name"]) {
  Some(Text(s)) => s,
  _ => "unnamed",
}
```

A document is its root table: its keys and values, in the order written
(a table's keys are unique). Every rule of TOML 1.1 is checked, and a
document that breaks one is an `Err` saying where (`line:column: ...`):
keys and tables defined twice, tables extended once closed (inline tables,
tables of dotted keys reopened by a `[header]`), integers beyond 64 bits,
dates that don't exist (`2023-02-29`), control characters, ...
One difference: a Strata `Float` is finite, so `inf` and `nan` are errors.

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

A time of day: `07:32:00.999` (seconds may be left out: `07:32`).

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

TOML's four kinds of date and time: an offset date-time (all three), a
local date-time (no offset), a local date, a local time.

## Traits

### Toml

```rust
derivable(Record | Union) trait Toml {
  /// The value as TOML.
  fn to_toml(self) -> Result[TomlValue, String]
  /// The value as a TOML document, which is a table: a record's.
  fn write_toml(self) -> Result[String, String]
  /// The value as TOML; `path` says where it is, for messages (`""` at the
  /// top).
  derived fn to_toml_at(self, path: String) -> Result[TomlValue, String]
  /// A value read from TOML, every refinement checked; `path` says where it
  /// is, for messages (`""` at the top).
  derived fn from_toml_at(
    v: TomlValue,
    path: String,
    keys: UnknownKeys,
  ) -> Result[Self, String]
  /// A value read from TOML, every refinement checked.
  fn from_toml(v: TomlValue) -> Result[Self, String]
  /// `from_toml`, with what to do with keys a record doesn't have given.
  fn from_toml_with(v: TomlValue, keys: UnknownKeys) -> Result[Self, String]
  /// A value read from a TOML document (`Config.read_toml(text)`).
  fn read_toml(text: String) -> Result[Self, String]
}
```

A record or a sum type read from TOML and written to it (see above).

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

TOML text read as its root table.

### stringify

```rust
fn stringify(table: List[(String, TomlValue)]) -> String
```

A table as a TOML document: its values first, then a `[header]` section
for each table in it, and a `[[header]]` for each table of each array of
tables. A table inside an array of other values is written inline.

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
