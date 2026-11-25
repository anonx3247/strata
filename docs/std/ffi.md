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
type parameters, can't be derived.

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
