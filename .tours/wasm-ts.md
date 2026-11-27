---
title: wasm32 and derive TypeScript
---

# Why
The user chose WebAssembly as the way TypeScript calls Strata: one module
that Node, Bun, Deno and browsers all load with the standard WebAssembly
API, with no native code and no FFI. `derive Python for f` already exports
`f` as `f_json`, with values crossing as JSON. This PR builds a module of
those exports (`strata build --target wasm32`), adds `derive TypeScript for
f` (typed wrappers and the loader in `NAME.ts`), and adds a case and an
example in which a function with a contract is called from TypeScript under
node.

# Background
`derive Python` is a function trait: a derived `json` (`json_call`)
exported as `NAME_json`, and a part of `NAME.py`. `build --lib` links the IR
with core.rt, the C runtime and std's precompiled library, and writes the
files from the IR's `; file` lines.

```strata std.ffi.Python
```

```strata std.ffi.json_call
```

# Intuition
codegen, core.rt and runtime.c all lay values out with 8-byte pointers. So
the same IR is compiled for **wasm64**, and binaryen then lowers the module
to a 32-bit memory. Pointers stay 8 bytes in memory and addresses are
wrapped, so it loads wherever plain wasm32 does. The C runtime keeps only
its portable part. A small C library (`runtime/wasm.c`) and five host
imports, which the generated loader provides, cover the rest. Calling
`clamp(1, 10, 0)` from TypeScript writes `{"x":1,"lo":10,"hi":0}` into the
module's memory and calls `clamp_json`. The broken `requires` reaches
core.rt's fault, which writes its message to the host and ends through
`strata_wasm_exit`. The loader throws that as a `StrataError` and starts a
fresh instance for the next call.

# The module
`compiler/wasm` finds the tools and makes the module. `wasm_built` is the
build command's new branch: std is compiled in, and only exports are built.

```strata-diff wasm.module wasm.tools wasm_built build_command
```

```strata-also
wasm.*: the tools, the features, running each
driver.valued_flags: `--target` takes a value
driver.usage_text: the help
use*wasm: the root module's import
```

# A fault reaches TypeScript
On arm64 an `i64` passed where the callee takes an `i32` goes unnoticed.
In wasm, wasm-ld replaces the call with a trapping stub, so every fault in a
module was a bare `unreachable`.

```strata-diff codegen.fault
```

```strata-also
codegen.checked codegen.decreases_fault codegen.regex_matches: the same i32 kind
```

# derive TypeScript
The trait and its file. The loader is one paragraph, written once per file
like the Python head. Records and union variants become interfaces tagged
by `"type"`, as their JSON is.

```strata-diff std.ffi.TypeScript std.ffi.typescript std.ffi.ts_type std.ffi.ts_function
```

```strata-also
std.ffi.ts_head std.ffi.ts_record std.ffi.ts_hint std.ffi.ts_doc: the loader, interfaces, types, docs
```

# The example
`examples/wasm`: `clamp` (with a contract), `summary` (a record, an `Err`)
and `grade` (a union, an `Option` parameter), called by `test.ts`.

```strata-also
clamp summary grade Summary Grade test* use*: examples/wasm
```
