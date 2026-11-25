# std.process

The running program: its arguments, its environment
variables and its exit code, and the other programs it runs.

`main` says what it needs, and the runtime passes it in:

```rust
fn main(mut console: Console, process: Process) -> ExitCode { ... }
```

`main` may return `()` (exit code 0), an `ExitCode`, or a `Result` of
either: an `Err` is written to standard error and the exit code is 1.

## Types

### ExitCode

```rust
type ExitCode = Int
  where 0 <= it && it <= 255
```

A process exit code: 0 for success, anything else for failure.

### Process

```rust
capability Process {}
```

The running process. Only the runtime makes one, for `main`.

### FakeEnvironment

```rust
type FakeEnvironment = {
  arguments: List[String],
  variables: Dict[String, String],
}
```

An environment for tests.

### RunMode

```rust
type RunMode =
  // `strata run`, or a built executable
  | Running
  // `strata test`
  | Testing
  // `strata run --trace`
  | Tracing
```

How the program is being run. `main` (or a test) may ask for it only when
strata.toml says `mode = true` under `[run]`: by default a program can't
tell, so it behaves the same wherever it runs.

### Finished

```rust
type Finished = { code: Int, out: String, err: String }
```

What a program that ran to its end left: its exit code (128 + the signal
for one killed by a signal), and what it wrote to standard output and to
standard error.

### Launcher

```rust
capability Launcher {}
```

The real system's programs. Only the runtime makes one, for `main` (or a
test that names it).

### RunOptions

```rust
type RunOptions = {
  /// What it reads on its standard input (`""`: nothing).
  input: String = "",
  /// Environment variables set for it (`("GIT_DIR", "x")`), on top of this
  /// program's own.
  env: List[(String, String)] = [],
  /// It uses this program's terminal: its standard input, output and error
  /// (an editor, or output shown as it comes). `Finished` then holds only the
  /// exit code.
  terminal: Bool = false,
}
```

How a program is run, besides its arguments and folder.

## Traits

### Environment

```rust
capability trait Environment {
  /// The command-line arguments, without the program's own name.
  fn args(self) -> List[String]
  /// The value of an environment variable, if it is set.
  fn var(self, name: String) -> Option[String]
  /// The folder the program runs in (its current folder), absolute.
  fn current_dir(self) -> String
}
```

What the program was started with: the real process, or a stand-in for it.

### Commands

```rust
capability trait Commands {
  /// Runs `program` (looked up in `PATH` unless it holds a `/`) with `args`,
  /// never through a shell, in the folder `dir` (`""`: the current one), with
  /// nothing on its standard input, and waits for it to finish. `Err` when it
  /// can't be started (no such program, or no such folder); a program that
  /// starts and fails is `Ok`, with its exit code.
  fn run(
    mut self,
    program: String,
    args: List[String],
    dir: String,
  ) -> Result[Finished, String]
    ensures match it {
      Ok(f) => 0 <= f.code,
      Err(_) => true,
    }
  /// `run`, with its standard input, environment and terminal as `options`
  /// say.
  fn run_with(
    mut self,
    program: String,
    args: List[String],
    dir: String,
    options: RunOptions,
  ) -> Result[Finished, String]
    ensures match it {
      Ok(f) => 0 <= f.code,
      Err(_) => true,
    }
}
```

Starting other programs: the real system (`Launcher`), or a stand-in in a
test (`mock Commands`, or a mock type that answers the calls a test
expects).

## Functions

### fake_environment

```rust
fn fake_environment(
  arguments: List[String],
  variables: List[(String, String)],
) -> FakeEnvironment
```

A test environment with these arguments and variables.

## Implementations

- `impl Environment for Process`
- `impl Environment for FakeEnvironment`
- `impl Commands for Launcher`
