# std.io

The console, text in and out.
`main` asks for a `Console`. It is the terminal under `strata run`, and the test's own output in a test.

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

A console for tests. It reads `input` line by line.
`out` is standard output as written. `err` holds the lines written to standard error.

### ReplConsole

```rust
capability ReplConsole {}
```

The console of code typed at `strata repl`. Only `strata repl` makes one.
Its output, standard error included in order, is shown with the line's result. `read_line` reads the REPL's next input.

## Traits

### Console

```rust
capability trait Console {
  /// Writes `text` and a newline to standard output.
  fn print(mut self, text: String)
  /// Writes `text` to standard output, without a newline.
  fn write(mut self, text: String)
  /// Writes `text` and a newline to standard error.
  fn eprint(mut self, text: String)
  /// The next line of standard input without its line ending. `None` at the end of the input.
  fn read_line(mut self) -> Option[String]
  /// The rest of standard input. A recorder gives its lines, each with a newline.
  fn read_all(mut self) -> String
  /// Whether standard input is a terminal rather than a pipe or a file. False for a stand-in.
  fn interactive(self) -> Bool
  /// A paint for `stream` (std.term), on when it is a terminal that shows colors. Off for a stand-in.
  fn paint(self, stream: Stream) -> Paint
}
```

Text output and input: the terminal, a `Recorder`, or a mock.

## Functions

### recorder

```rust
fn recorder(input: List[String]) -> Recorder
```

A recorder that reads `input` line by line.

### printed

```rust
fn printed(r: Recorder) -> List[String]
```

The lines of a recorder's standard output.

## Implementations

- `impl Console for Terminal`
- `impl Console for Recorder`
- `impl Console for ReplConsole`
