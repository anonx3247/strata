# std.ffi

Strata functions called from other languages.

A trait derivable over functions (`derivable(Function)`) says what
exporting a function to a language makes; `derive C for add` makes `add`
a function C code calls. A library built with `strata build --lib`
exports it under its own name, with C's types, and declares it in the
library's header (`<name>.h`):

```rust
from std.ffi use C

/// The sum of two numbers.
pub fn add(a: I32, b: I32) -> I32 = a + b

derive C for add
```

declares

```rust
/// The sum of two numbers.
int32_t add(int32_t a, int32_t b);
```

The types that cross are `extern c`'s: the sized integers (`I8` … `U64`
as `int8_t` … `uint64_t`), `Float` (`double`), `Bool` (`bool`), `String`
(a NUL-terminated copy: `const char *` in, a `char *` out that the caller
frees with `free`), `()` (`void`), and `extern c type` handles (`void *`;
`Option` of one: NULL for `None`). A function with another type, or with
type parameters, can't be derived. `derive C for module` derives it for
every `pub fn` of the module (one that can't cross is reported).

A library exports only what its derives export, and
`strata_string_free(char *)`, which frees a string they return: the rest
(the program's other functions, std's, the runtime's) is hidden, so two
Strata libraries live in one process.

`derive Python for f` exports `f` through JSON, and writes a typed
Python module beside the library (`<name>.py`):

```rust
from std.ffi use Python
from std.json use Json

/// A point on the plane.
type Point = { x: Float, y: Float }

derive Json for Point

/// The middle of two points.
pub fn middle(p: Point, q: Point) -> Point { ... }

derive Python for middle
```

```python
import geo                       # geo.py, beside libgeo.dylib
geo.middle(geo.Point(0.0, 0.0), geo.Point(2.0, 4.0))   # Point(x=1.0, y=2.0)
```

The export is `char *middle_json(const char *args)`: `args` a JSON object
of the arguments by name, each read with its type's `Json` (its
refinements checked; an `Option` left out is `None`; an unknown one
refused), the answer `{"ok": RESULT}` or `{"error": {"kind": K,
"message": M}}` (with `"value"`: the function's own `Err`, as JSON), never
a crash. `K` is `"json"` (not JSON), `"arguments"` (not an object; an
argument missing, unknown, of the wrong type or breaking a refinement),
`"result"` (a result JSON can't hold: an integer past 2^53) or `"err"`
(the function returned `Err`). The caller frees the answer with
`strata_string_free`. Every type crossing must be `Json` (records and
unions derive it). In Python: a function per derived one, with type hints;
a dataclass per record; a class per union with a subclass per variant
(`Circle(Shape)`), a union of atoms a `Literal` of their names; an error
answer raised as `StrataError` (`kind`, `message`, `value`). `json_call`
is the export (`derive TypeScript`, next, exports the same).

## Traits

### C

```rust
derivable(Function) trait C {
  /// The function itself, exported under its own name (its arguments and
  /// result converted as an `extern c` function's are, the other way).
  derived fn export() -> Self
  /// Its declaration in the library's header (`<name>.h`), after its doc.
  derived fn h() -> String
}
```

A function C code calls: exported, when the program is built as a
library (`strata build --lib`), under its own name with C's types, and
declared in the library's header.

### Python

```rust
derivable(Function) trait Python {
  /// `char *NAME_json(const char *args)`: the function called with its
  /// arguments as a JSON object, its answer as JSON (`json_call`).
  derived fn json(args: String) -> String
  /// Its part of the library's Python module (`<name>.py`).
  derived fn py() -> String
}
```

A function Python calls, through JSON (above).

## Functions

### json_call

```rust
pub fn json_call(comptime f: Type, args: String) -> String
```

The function `f` called with `args`, a JSON object of its arguments, and
its answer as JSON (above): the body of `derive Python`'s `NAME_json`.
