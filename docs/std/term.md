# std.term

Text styled for a terminal with ANSI escape codes.
A paint from `console.paint(stream)` is on only for a terminal. Off, it returns text unchanged.

## Types

### Stream

```rust
type Stream =
  | Output
  | Errors
```

Standard output or standard error.

### Paint

```rust
type Paint = { on: Bool }
```

Styles text with ANSI escape codes when `on`.

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

Whether to color a stream. On when `FORCE_COLOR` is set.
Otherwise on for a terminal with `NO_COLOR` unset and `TERM` not `dumb`.

### styled

```rust
fn styled(p: Paint, code: String, text: String) -> String
```

`text` wrapped in the escape code `code` (`1` bold, `31` red, …) and a reset. Unchanged when the paint is off.

### plain

```rust
fn plain(text: String) -> String
```

`text` without its escape codes (`ESC [ … letter`), as a terminal shows it.
