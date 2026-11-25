# std.code

std.code: a codebase as Strata's tools see it, for viewers and agents.
The codebase is the current folder, a package with its log.

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

One definition of a codebase.

- `name` is qualified by its module (`core.run`). `label` is a test's description, else the name.
- `kind` is `fn`, `type`, `trait`, `impl`, `test` or `use`.
- `interface` is the signature and contracts, or a whole type or trait. `source` is the whole definition. Neither holds the doc.
- `deps` are the definitions it uses, `refs` those that use it, `impls` the `impl` blocks of a type or trait.
- `verdict` is the last valid result of `strata verify` (`proven`, `fuzzed`, …).
- `last_change` is the index in `log` of the entry that last changed it.

### Entry

```rust
type Entry = { message: String, time: Option[Int], changes: List[Change] }
```

One `strata edit` or other change in a package's log.
`time` is in seconds since 1970. `what` of a change is `added`, `removed`, `interface` or `body`.

### Change

```rust
type Change = { name: String, what: String }
```

### Codebase

```rust
capability Codebase {}
```

The codebase `strata` is looking at. Only the runtime makes one.

## Traits

### Code

```rust
capability trait Code {
  /// Cheap. Read the snapshot again only when it changed.
  fn version(self) -> String
  /// Fails when a file doesn't decode.
  fn snapshot(self) -> Result[Snapshot, String]
}
```

Reads a codebase.

## Implementations

- `impl Code for Codebase`
