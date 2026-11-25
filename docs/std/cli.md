# std.cli

Command-line options, read into a record; commands, into a union.

Describe the options as a record, and derive `CommandLine` for it:

```rust
/// Count lines, words and characters.
type Options = {
  /// Count lines only.
  lines: Bool,
  /// -n Stop after this many files.
  limit: Option[Positive],
  /// The files to count.
  files: List[String],
}

derive CommandLine for Options

fn main(opts: Options) { ... }
```

`strata run` (and a compiled program) builds the options from the
arguments with `Options.from_args`, checks every refinement, and answers
`--help` from the doc comments. Each field becomes:

- `Bool`: a flag, `--lines`, false unless given;
- `Option[T]`: an option, `--limit 3` (or `--limit=3`), `None` unless
  given; or an optional positional argument when its doc starts with its
  name in brackets (`/// [module] The module to show.`), filled after the
  required ones, from the arguments before a `--`;
- `List[T]`: the remaining positional arguments, in order (one such
  field); or, when its doc starts with its flag or a short name
  (`/// --rm A definition to remove.`), an option that may be given
  again and again (`--rm a --rm b`);
- a union that derives `CommandLine` (see below): a command, named by the
  first positional argument, which takes all the arguments after it: the
  record's other fields are options given before it;
- anything else: a required positional argument, in field order, or an
  option (`--times 3`) when the field has a default (`times: Int = 1`).

A field with a default takes it when it is left out (a flag's default can
be `true`; a list's default is used when no arguments are left).
`T` is an `Int`, a `Float`, a `String`, a refinement of one, or a choice: a
type whose variants carry nothing (`type Level = | Quiet | Loud` is
`quiet` or `loud`). Names are written with dashes: `dry_run` is `--dry-run`.
An option or flag whose doc starts with `-` and a letter (`/// -n Stop
after…`) can also be given by that short name: `-n 3`.

Commands: derive `CommandLine` for a union whose members are records (or
carry nothing), one per command, named as the member is (`DryRun` is
`dry-run`): the first argument names the command, the rest are read into
that member's fields as above, and `--help` lists the commands with their
doc comments (`tool edit --help`, a command's own options).

```rust
type Command =
  /// Show the history.
  | Log
  /// Replace definitions.
  | Edit {
    /// [module] The module to change.
    module: Option[String],
    /// --rm A definition to remove (again and again).
    rm: List[String] = [],
  }

derive CommandLine for Command
```

`read_input(mut console, files, path)` reads a file, or standard input when
the path is `-`.

## Types

### CliError

```rust
type CliError =
  | ShowHelp { text: String }
  | BadArgs { message: String }
```

The help asked for (`--help`), or what is wrong with the arguments.

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
  /// Its doc comment, on one line (without its `-m` or `[name]`).
  doc: String,
  /// What `--help` says of it.
  text: String,
  choice: Bool,
  /// A command's: each command's name and doc.
  commands: List[(String, String)],
}
```

One field, as the parser and `--help` see it.

### Given

```rust
type Given = { values: List[(String, List[String])] }
```

What the arguments gave each field: its texts, in order (a field given
again and again has an entry each time).

## Traits

### CommandLine

```rust
derivable(Record | Union) trait CommandLine {
  /// The options, from the program's arguments: the help when they ask for
  /// it, or what is wrong with them.
  derived fn from_args(
    program: String,
    args: List[String],
  ) -> Result[Self, CliError]
}
```

A record whose fields are a program's command-line options, or a union of
its commands.

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

What `--help` prints, its section headings (`Usage:`, `Options:`, …)
in bold when the paint is on.

### error_line

```rust
fn error_line(message: String, p: Paint) -> String
```

`error: message`, as a program shows what went wrong (`error:` in red,
bold, when the paint is on).
