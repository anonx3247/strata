# std.code

A codebase as Strata's tools see it: its definitions, each
rendered as text with what it uses and what uses it, the checker's
findings, the verifier's cached results and the package's log.

```rust
from std.code use Codebase, Snapshot

fn main(code: Codebase) {
  match code.snapshot() {
    Ok(s) => print(f"{len(s.definitions)} definitions in {s.label}"),
    Err(e) => print(e),
  }
}
```

The codebase is the current folder (a package, with its log), or the one
`strata ui` shows: its viewer is a program that asks for `Code`.

## Types

### Snapshot

```rust
type Snapshot = {
  version: String,
  label: String,
  definitions: List[Definition],
  errors: List[String],
  log: List[Entry],
}
```

A codebase at one moment. `version` changes whenever its files do.

### Definition

```rust
type Definition = {
  name: String,
  module: String,
  label: String,
  kind: String,
  public: Bool,
  doc: String,
  hash: String,
  interface: String,
  source: String,
  deps: List[String],
  refs: List[String],
  impls: List[String],
  holes: List[String],
  errors: List[String],
  verdict: Option[String],
  last_change: Option[Int],
}
```

One definition. `name` is qualified by its module (`core.run`); `label`
is what to call it (a test's description). `kind` is `fn`, `type`,
`trait`, `impl`, `test` or `use`. `interface` is its signature and
contracts (a type or trait whole), `source` the whole definition, both
without the doc comment (`doc`). `deps` are the definitions it uses,
`refs` those that use it, `impls` the `impl` blocks of a type or trait.
`verdict` is what `strata verify` last found (`proven`, `fuzzed`, …) if
it is still valid; `last_change` the index in `log` of the entry that
last changed it.

### Entry

```rust
type Entry = { message: String, time: Option[Int], changes: List[Change] }
```

An entry of a package's log: one `strata edit` (or other change), when
(seconds since 1970), and the definitions it `added`, `removed`, or
changed (`interface` or `body`).

### Change

```rust
type Change = { name: String, what: String }
```

### Codebase

```rust
capability Codebase {}
```

The codebase strata is looking at: the current folder, or the one
`strata ui` shows. Only the runtime makes one.

## Traits

### Code

```rust
capability trait Code {
  /// What changes whenever the code does: cheap to ask, to read the
  /// snapshot only when it changed.
  fn version(self) -> String
  /// The codebase now, or why it can't be read (a file that doesn't decode).
  fn snapshot(self) -> Result[Snapshot, String]
}
```

The capability to read a codebase.

## Implementations

- `impl Code for Codebase`
