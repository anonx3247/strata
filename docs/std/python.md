# std.python

Calls to Python functions declared `extern python "module" fn …`.
A module wraps them in a capability, so a signature shows what code can reach Python.

## Types

### PythonError

```rust
type PythonError =
  /// The function raised an exception, or couldn't be found (`ImportError`, `AttributeError`).
  /// `exception` is the type's name, such as `ValueError`.
  | Raised { exception: String, message: String, traceback: String }
  /// The worker couldn't start (no `python3`) or ended while answering. The next call starts a new one.
  | WorkerFailed { message: String }
  /// The result isn't the declared type. `message` is std.json's.
  | BadReply { message: String }
  /// An argument has no JSON form, such as an integer beyond 2^53.
  | NotEncodable { message: String }
```

Why a call of a Python function failed.

- Values cross as JSON through the derived `Json` trait. The result is read with the declared type's `from_json`, refinements checked.
- A function declared to return `Result[(), PythonError]` may return anything.
- Calls go to one Python worker process, started on the first call and kept for the run. It runs `python3` from `PATH`, or `[python] executable` in strata.toml.
- Modules are found in the package's folder and the folders of `[python] path`, before Python's own path.
- Calls from several tasks take turns on the worker.
- What the Python code prints goes to standard error. Standard output holds the replies.
- `verify` never runs a Python function. Its contracts are checked when it runs.
