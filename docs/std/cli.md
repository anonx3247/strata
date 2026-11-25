# std.cli

std.cli: command-line options read into a record, and commands into a union, with `derive CommandLine`.
`strata run` and a compiled program build `main`'s options from the arguments and answer `--help` from the doc comments.

## Types

### CliError

```rust
type CliError =
  | ShowHelp { text: String }
  | BadArgs { message: String }
```

The help asked for with `--help`, or what is wrong with the arguments.

### OptKind

```rust
type OptKind =
  | FlagOpt
  | OptionalOpt
  | DefaultedOpt
  | RepeatedOpt
  | RestOpt
  | PositionalOpt
  | MaybeOpt
  | CommandOpt
```

How a field is given on the command line.

### OptSpec

```rust
type OptSpec = {
  name: String,
  flag: String,
  /// Its short name (`m` for `-m`), or `""`.
  short: String,
  kind: OptKind,
  placeholder: String,
  /// Its doc comment on one line, without its `-m` or `[name]`.
  doc: String,
  /// What `--help` says of it.
  text: String,
  choice: Bool,
  /// For a command field, each command's name and doc.
  commands: List[(String, String)],
}
```

One field, as the parser and `--help` see it.

### Given

```rust
type Given = { values: List[(String, List[String])] }
```

The texts the arguments gave each field, in order. A field given several times has an entry each time.

## Traits

### CommandLine

```rust
derivable(Record | Union) trait CommandLine {
  /// The options from the program's arguments, every refinement checked. `Err` holds the help when asked for, or what is wrong.
  derived fn from_args(
    program: String,
    args: List[String],
  ) -> Result[Self, CliError]
}
```

A record whose fields are a program's command-line options, or a union of its commands.
Each field of a record becomes:

- `Bool`: a flag, `--lines`, false unless given.
- `Option[T]`: an option, `--limit 3` or `--limit=3`, `None` unless given. When its doc starts with its name in brackets (`/// [module] The module.`), an optional positional argument instead, filled after the required ones from the arguments before a `--`.
- `List[T]`: the remaining positional arguments, in order. Only one field can be this. When its doc starts with its flag (`/// --rm A definition.`), an option that may be repeated: `--rm a --rm b`.
- a union that derives `CommandLine`: a command, named by the first positional argument, which takes all the arguments after it. The record's other fields are options given before it.
- anything else: a required positional argument in field order, or an option (`--times 3`) when the field has a default.

Other rules:

- A field left out takes its default. A flag's default can be `true`. A list's default is used when no arguments are left.
- `T` is an `Int`, a `Float`, a `String`, a refinement of one, or a `Choice`: `type Level = Quiet | Loud` is `quiet` or `loud`.
- Names are written with dashes: `dry_run` is `--dry-run`.
- An option or flag whose doc starts with `-` and a letter (`/// -n Stop after…`) also has that short name: `-n 3`.
- In a union of commands, each member is a record or an atom, named in kebab case (`DryRun` is `dry-run`). `--help` lists the commands, and `tool edit --help` a command's own options.

## Functions

### kebab

```rust
fn kebab(name: String) -> String
```

`DryRun` -> `dry-run`

### parse

```rust
fn parse(
  program: String,
  specs: List[OptSpec],
  args: List[String],
  check: (String, String) -> Result[(), String],
  help: String,
) -> Result[Given, CliError]
  terminates
```

Read the arguments with the options' descriptions; `check` checks one
field's text as the parser meets it.

### help_text

```rust
fn help_text(
  program: String,
  doc: List[String],
  specs: List[OptSpec],
) -> String
```

What `--help` prints.

### read_input

```rust
fn read_input[C: Console, F: Files](
  mut console: C,
  files: F,
  path: String,
) -> Result[String, FsError]
```

A file's text, or standard input's when `path` is `-`.

### commands_help

```rust
fn commands_help(
  program: String,
  doc: List[String],
  commands: List[(String, String)],
) -> String
```

What `--help` prints for a union of commands.

### styled_help

```rust
fn styled_help(text: String, p: Paint) -> String
```

What `--help` prints, with its section headings (`Usage:`, `Options:`, …) in bold when the paint is on.

### error_line

```rust
fn error_line(message: String, p: Paint) -> String
```

`error: message`, with `error:` in bold red when the paint is on.
