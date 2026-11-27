# std.ts

Calls to TypeScript functions declared `extern ts "module" fn …`.
A module wraps them in a capability, so a signature shows what code can reach TypeScript.

## Types

### TsError

```rust
type TsError =
  /// The function threw, or couldn't be found (a module or a function missing).
  /// `name` is the error's name, such as `RangeError`; `stack` its stack, if it has one.
  | Thrown { name: String, message: String, stack: String }
  /// The worker couldn't start (no `node`) or ended while answering. The next call starts a new one.
  | WorkerFailed { message: String }
  /// The result isn't the declared type, or has no JSON form (a `bigint`, `NaN`). `message` says why.
  | BadReply { message: String }
  /// An argument has no JSON form, such as an integer beyond 2^53.
  | NotEncodable { message: String }
```

Why a call of a TypeScript function failed.

- Values cross as JSON through the derived `Json` trait. The result is

read with the declared type's `from_json`, refinements checked.

- A function declared to return `Result[(), TsError]` may return

anything. A function returning a promise is awaited.

- Calls go to one worker process, started on the first call and kept

for the run. It runs `node` from `PATH`, or `[ts] executable` in
strata.toml (`bun` and `deno` work too). Node runs `.ts` files by
stripping their types (Node 22.18 and later).

- A module `"text_utils"` is `text_utils.ts` (or `.mts`, `.js`, `.mjs`)

in the package's folder or the folders of `[ts] path`, else a package in
their `node_modules`.

- Calls from several tasks take turns on the worker.
- What the code prints (`console.log`) goes to standard error. Standard

output holds the replies.

- `verify` never runs a TypeScript function. Its contracts are checked

when it runs.
