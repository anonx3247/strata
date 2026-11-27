# std.ffi

Strata functions called from other languages.
`derive C for add` or `derive Python for add` exports `add` from a library built with `strata build --lib`.

## Traits

### C

```rust
derivable(Function) trait C {
  /// The function, with its arguments and result converted as an `extern c` function's are, the other way.
  derived fn export() -> Self
  /// Its declaration in the header, after its doc.
  derived fn h() -> String
}
```

A function C code calls. A library built with `strata build --lib` exports it under its own name and declares it in the library's header, `<name>.h`.
The types that cross are `extern c`'s:

- `I8` … `U64` as `int8_t` … `uint64_t`, `Float` as `double`, `Bool` as `bool`, `()` as `void`
- `String` as a NUL-terminated copy: `const char *` in, and a `char *` out that the caller frees with `free`
- an `extern c type` handle as `void *`, and an `Option` of one as NULL for `None`

A function with another type, or with type parameters, can't be derived.

### Python

```rust
derivable(Function) trait Python {
  /// `char *NAME_json(const char *args)`. The caller frees the answer with `strata_string_free`.
  derived fn json(args: String) -> String
  /// Its part of `<name>.py`: the module's head, a class for each record and union it takes or gives, and the function.
  derived fn py() -> String
}
```

A function Python calls. A library built with `strata build --lib` exports it as `NAME_json`, which takes and returns JSON (`json_call`).
The library's Python module, `<name>.py`, wraps it in a typed function, with a class for each record and union that crosses.

### TypeScript

```rust
derivable(Function) trait TypeScript {
  /// `NAME_json(args)`, a pointer to a NUL-terminated string in the module's memory each way. The loader frees the answer with `strata_string_free`.
  derived fn json(args: String) -> String
  /// Its part of `<name>.ts`: the loader, a type for each record and union it takes or gives, and the function.
  derived fn ts() -> String
}
```

A function TypeScript (or JavaScript) calls, from a WebAssembly module
built with `strata build --target wasm32`, which exports it as
`NAME_json`, taking and returning JSON (`json_call`), as `derive Python`
does. The module's TypeScript file, `<name>.ts`, loads `<name>.wasm`
beside it with the standard WebAssembly API and wraps each function in a
typed one, with a type for each record and union that crosses, so it runs
in Node, Bun, Deno and browsers with no native code.

## Functions

### json_call

```rust
fn json_call(comptime f: Type, args: String) -> String
```

Calls `f` with `args`, a JSON object of its arguments by name, and returns its answer as JSON.

- Each argument is read with its type's `Json`, refinements checked. An `Option` left out is `None`.
- The answer is `{"ok": RESULT}` or `{"error": {"kind": K, "message": M}}`.
- K is `"json"` when `args` isn't JSON, and `"arguments"` when it isn't an object or an argument is missing, unknown, of the wrong type or breaks its refinements.
- K is `"result"` for a result JSON can't hold, such as an integer past 2^53.
- K is `"err"` for the function's own `Err`, whose value is also in `"value"` as JSON.
