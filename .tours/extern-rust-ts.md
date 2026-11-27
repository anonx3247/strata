---
title: extern rust and extern ts
---

# Why
Strata calls C (`extern c`) and Python (`extern python`). Rust crates and
TypeScript modules are the other two ecosystems agents reach for, and the plan
(IDEAS.md "Foreign functions: `extern LANG`") had them follow Python: values
cross as JSON through `derive Json`, the module named on each declaration,
results written `Result[T, RustError]` / `Result[T, TsError]`, only
capabilities call them, `verify` never runs them. This PR builds both, so the
three read alike.

# Background
`extern python` is a declaration that `desugar` turns into an ordinary
function calling std.python: arguments to JSON, a request to a worker process
started by the runtime, the reply decoded against the declared type.
`capabilities` checks the rules, `verify` takes the generated function as
assumed. Nothing else in the compiler knows Python.

```strata ast.Lang
```

```strata std.python.call
```

# Intuition
`extern ts "text_utils" fn slugify(title: String) -> Result[String, TsError]`
becomes `std.ts.decoded(std.ts.call("text_utils", "slugify",
[std.ts.argument(title)]))`: exactly Python's shape with another std module.
TypeScript runs as Python does, in a `node -e DRIVER` worker. Rust is
compiled, so it runs in the process: for `extern rust "text_tools" fn
add(a: Int, b: Int)` the compiler writes a crate with
`#[no_mangle] extern "C" fn strata_rust_text_tools__add(args: *const c_char)`
that reads `[40, 2]` with serde_json, calls `text_tools::add(40, 2)` and
answers `{"ok": 42}`; cargo builds it into a library the runtime `dlopen`s.

# One foreign-language table
Each foreign language is a `Lang` variant with a string; `foreign_of` gives
its keyword, name, std module and error type, and every pass that knew Python
asks it instead.

```strata-diff ast.Lang ast.foreign_of ast.Foreign desugar.python_lowered desugar.foreign_call
```

```strata-also
desugar.python_call: kept, any foreign call now
desugar.use*: imports foreign_of
capabilities.python_errors capabilities.json_error capabilities.use*: the rules and messages for the three languages
parse.decl_kind parse.foreign_missing parse.use*: `extern rust "crate"`, `extern ts "module"`, and the error without one
text.decl_body_text text.use*: printing
binary.decl binary.put_decl: tags 3 and 4 (no file re-encoded)
checks_of verified_decls: verify's assumed message and hole per language
std_called: std.rust and std.ts functions called by generated code
```

# TypeScript: a Node worker
std.ts is std.python with `TsError` and the `ts.*` settings; the runtime's
worker code is shared (`Worker` in runtime.c), with a JavaScript driver.

```strata-diff std.ts.TsError std.ts.call
```

```strata-also
std.ts.*: the rest of the module, as std.python's
deps.package_settings deps.unknown_keys: `[ts]` and `[rust]` in strata.toml
settings_at settings_stamp: `ts.path` and `rust.library`, kept out of the stamp
```

# Rust: a crate cargo builds, loaded in the process
`cargo.shim` writes Cargo.toml and lib.rs; `cargo.build` runs cargo;
`rust_built` calls it from `run`, `test` and `build`. cargo makes a shared
library, which `run` and `test` load, and a static one, which `rust_linked`
links into a built executable, so the executable needs nothing from
`.strata` at run time. A Rust function
returning `Result<T, E>` answers its `Err` as `Failed` (autoref
specialization in the shim's head); panics are caught.

```strata-diff cargo.shim cargo.build rust_built rust_linked std.rust.RustError std.rust.call
```

```strata-also
cargo.*: the crate's location, its two libraries, the declarations, symbols, the shim's head
linked: the C compiler links a program with `extern rust`
std.rust.*: the rest of the module, as std.python's
run_command test_command build_command: build the crates first
use*cargo: the root module's import
```

# The examples
`examples/rust` (the crate `text_tools/`, serde only) and `examples/ts`
(`text_utils.ts`) are `examples/python` with the other two languages: the same
`TextTools` capability, records, a `Dict`, a refinement, and the error a
function gives for a text with no year.

```strata-also
text.* main test* use*text use*std.rust use*std.ts: examples/rust and examples/ts
```
