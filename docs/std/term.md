# std.term

Text styled for a terminal: bold, dim, colors.

Ask the console for a `Paint` for the stream you write to, then style
text with it:

```rust
paint := console.paint(Output)
mut console.print(paint.bold(paint.red("error:")) + " no such file")
```

A paint is on only when the stream is a terminal (not a pipe or a file),
`NO_COLOR` isn't set and `TERM` isn't `dumb` (`FORCE_COLOR` turns it on
anyway); off, it gives the text back as it is. A test's `Recorder` gives
a paint that is off, and `paint(true)` one that is on.

## Types

### Stream

```rust
type Stream =
  | Output
  | Errors
```

Where text goes: standard output or standard error.

### Paint

```rust
type Paint = { on: Bool }
```

Styles text with terminal escape codes, when `on`.

## Methods

### Paint.bold

```rust
fn bold(self, text: String) -> String
```

### Paint.dim

```rust
fn dim(self, text: String) -> String
```

### Paint.italic

```rust
fn italic(self, text: String) -> String
```

### Paint.underline

```rust
fn underline(self, text: String) -> String
```

### Paint.red

```rust
fn red(self, text: String) -> String
```

### Paint.green

```rust
fn green(self, text: String) -> String
```

### Paint.yellow

```rust
fn yellow(self, text: String) -> String
```

### Paint.blue

```rust
fn blue(self, text: String) -> String
```

### Paint.magenta

```rust
fn magenta(self, text: String) -> String
```

### Paint.cyan

```rust
fn cyan(self, text: String) -> String
```

### Paint.gray

```rust
fn gray(self, text: String) -> String
```

## Functions

### paint

```rust
fn paint(on: Bool) -> Paint
```

A paint: on (escape codes) or off (text as it is).

### colors_for

```rust
fn colors_for(
  terminal: Bool,
  no_color: Option[String],
  force_color: Option[String],
  term: Option[String],
) -> Bool
```

Whether a paint for `stream` is on, from the environment: a terminal,
`NO_COLOR` unset, `TERM` not `dumb`; or `FORCE_COLOR` set.

### styled

```rust
fn styled(p: Paint, code: String, text: String) -> String
```

`text` between the escape code `code` (`1` bold, `31` red, …) and the
reset, or `text` when the paint is off.

### plain

```rust
fn plain(text: String) -> String
```

The text without its escape codes (`ESC [ … letter`): what a terminal
shows, to measure or compare it.
