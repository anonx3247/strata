# std.io

The console: text in and out.

The console is a capability. A program gets it only by asking for it in
`main` (`fn main(mut console: Console)`), and passes it on to the code that
needs it. It is the one capability the environment chooses: the terminal
under `strata run`, the test's own output in a test. Tests may pass a
`Recorder` instead.

## Types

### Terminal

```rust
capability Terminal {}
```

The process's own terminal. Only the runtime makes one, for `main`.

### Recorder

```rust
type Recorder = { input: List[String], out: String, err: List[String] }
```

A console for tests. It reads its input from `input`, line by line, and
keeps what is written: `out` is standard output as written, `err` the
lines written to standard error.

## Traits

### Console

```rust
capability trait Console {
  /// Write `text` and a newline to standard output.
  fn print(mut self, text: String)
  /// Write `text` to standard output, without a newline.
  fn write(mut self, text: String)
  /// Write `text` and a newline to standard error.
  fn eprint(mut self, text: String)
  /// The next line of standard input, without its line ending; `None` at the
  /// end of the input.
  fn read_line(mut self) -> Option[String]
  /// The rest of standard input, whole: what `read_line` hasn't taken (a
  /// stand-in gives its lines, each with a newline).
  fn read_all(mut self) -> String
  /// Whether standard input is a terminal (a person typing), not a pipe or a
  /// file; a stand-in's is not.
  fn interactive(self) -> Bool
  /// A paint for `stream` (std.term): on when it is a terminal that shows
  /// colors; a stand-in's is off.
  fn paint(self, stream: Stream) -> Paint
}
```

Text output and input: the terminal, or a stand-in for it.

## Functions

### recorder

```rust
fn recorder(input: List[String]) -> Recorder
```

A recorder that will read `input`, line by line.

### printed

```rust
fn printed(r: Recorder) -> List[String]
```

The lines a recorder's standard output holds.

## Implementations

- `impl Console for Terminal`
- `impl Console for Recorder`
