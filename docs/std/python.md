# std.python

Calling Python functions (`extern python`).

A module declares the Python functions it calls, each with the Python
module it is in, and turns them into a capability, so a signature shows
what code can reach Python (`main` and tests ask for the capability):

```rust
from std.python use PythonError

/// A title as a URL slug (`text_utils.py`, beside strata.toml).
extern python "text_utils" fn slugify(title: String) -> Result[String, PythonError]

pub capability TextUtils {
  fn slug(title: String) -> Result[String, PythonError] {
    slugify(title)
  }
}
```

Values cross as JSON, through the derived `Json` trait (std.json): each
argument is written with `to_json`, the function called with them in
order, and what it returns read back with the declared type's
`from_json` (refinements checked). A call is a `Result`: a Python
exception, a worker that couldn't start or ended, a reply that isn't the
declared type (a `NaN`, an integer beyond 2^53, a missing field) are
`PythonError` values, never faults. A function declared to return
`Result[(), PythonError]` may return anything (`None`).

The calls go to one Python worker process, started on the first call and
kept for the rest of the run: `python3` on the `PATH`, or `[python]
executable` in strata.toml. Its modules are found in the package's folder
(a script's), or the folders of `[python] path` (relative to it), before
Python's own path. Calls from several tasks take turns on it. What the
Python code prints goes to standard error (standard output carries the
replies). `verify` never runs a Python function: it is assumed, its
contracts checked when it runs.

## Types

### PythonError

```rust
type PythonError =
  /// The function raised an exception (or couldn't be found: an
  /// `ImportError`, an `AttributeError`): its type's name (`ValueError`),
  /// its message, and Python's traceback.
  | Raised { exception: String, message: String, traceback: String }
  /// The Python worker couldn't be started (no `python3`), or it ended
  /// while answering (killed, crashed, `os._exit`); the next call starts a
  /// new one.
  | WorkerFailed { message: String }
  /// What the function returned isn't the declared result: what reading it
  /// as one found (std.json's message).
  | BadReply { message: String }
  /// An argument has no JSON form (an integer beyond 2^53).
  | NotEncodable { message: String }
```

Why a call of a Python function failed.
