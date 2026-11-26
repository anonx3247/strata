# std.rust

Calls to Rust functions declared `extern rust "crate" fn …`.
A module wraps them in a capability, so a signature shows what code can reach Rust.

## Types

### RustError

```rust
type RustError =
  /// The function returned `Err(e)`. `message` is `e.to_string()`.
  | Failed { message: String }
  /// The function panicked. `message` is the panic's.
  | Panicked { message: String }
  /// The arguments aren't what the Rust function takes. `message` is serde's.
  | BadArguments { message: String }
  /// The library couldn't be loaded, or has no such function.
  | NotLoaded { message: String }
  /// The result isn't the declared type. `message` is std.json's.
  | BadReply { message: String }
  /// An argument has no JSON form, such as an integer beyond 2^53.
  | NotEncodable { message: String }
```

Why a call of a Rust function failed.

- The crate is a folder with a Cargo.toml, relative to the package

(`"text_tools"`, `"crates/text"`), or a crate of crates.io by name
(`"name@1.2"`); `"text_tools::words"` names a module in it.

- `strata run`, `test` and `build` write a crate calling the declared

functions into `.strata/rust` in the package and build it with `cargo
build --release` (cargo from `PATH`, or `[rust] cargo` in strata.toml).
A built executable has the static library linked in; `run` and `test`
load the shared one on the first call, in the process.

- Values cross as JSON through the derived `Json` trait and serde: the

Rust parameters are `serde::Deserialize`, the result `serde::Serialize`.
The result is read with the declared type's `from_json`, refinements
checked.

- A Rust function returning `Result<T, E>` (`E: Display`) answers its

`Err` as `Failed`. A panic is caught, as `Panicked`.

- A function declared to return `Result[(), RustError]` may return

anything.

- Calls from several tasks run at once.
- `verify` never runs a Rust function. Its contracts are checked when it

runs.
