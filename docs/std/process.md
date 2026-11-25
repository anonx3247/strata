# std.process

std.process: the running program's arguments, environment and exit code, and the programs it runs.
`main` may return `()` (exit code 0), an `ExitCode`, or a `Result` of either. An `Err` is written to standard error and exits with 1.

## Types

### ExitCode

```rust
type ExitCode = Int
  where 0 <= it && it <= 255
```

A process exit code from 0 to 255. 0 is success.

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

How the program is being run.
`main` or a test may ask for it only when strata.toml sets `mode = true` under `[run]`. Otherwise a program behaves the same wherever it runs.

### Finished

```rust
type Finished = { code: Int, out: String, err: String }
```

A finished program's exit code and output.
A program killed by a signal has code 128 + the signal.

### Launcher

```rust
capability Launcher {}
```

The real system's programs. Only the runtime makes one, for `main` or a test that names it.

### RunOptions

```rust
type RunOptions = {
  /// Its standard input. `""` is none.
  input: String = "",
  /// Environment variables set on top of this program's own, such as `("GIT_DIR", "x")`.
  env: List[(String, String)] = [],
  /// Gives it this program's terminal, for an editor or output shown as it comes. `Finished` then holds only the exit code.
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
  /// The current folder, absolute.
  fn current_dir(self) -> String
}
```

What the program was started with: the real process or a stand-in.

### Commands

```rust
capability trait Commands {
  /// Runs `program` with `args` in the folder `dir` and waits for it.
  /// - `program` is looked up in `PATH` unless it holds a `/`. No shell is involved.
  /// - `dir` is `""` for the current folder.
  /// - Its standard input is empty.
  /// - `Err` when it can't be started: no such program or folder. A program that starts and fails is `Ok` with its exit code.
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
  /// `run`, with standard input, environment and terminal set by `options`.
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

Starts other programs: `Launcher` for the real system, or a mock in a test.

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
