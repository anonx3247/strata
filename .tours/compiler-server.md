---
title: A long-running compiler, `strata daemon`
packages: compiler
---

# Why
Every `strata` command loads the package, std and core from nothing. `check`
of `compiler/` takes about 5 s, even when one function changed since the
last check. The goal (IDEAS, "Speed") is well under a second. A process that
stays up can keep what it loaded, read only the files that changed, and
check again only what they can affect.

Measured on `compiler/`, with the machine loaded: cold `check` 4.7 to 5.3 s.
With the daemon: 0.02 s when nothing changed, 0.43 s after a one-function
edit, 0.18 s on a second edit of the same function. Resident size: about
950 MB.

# Background
A command is `command_line`, then `strata_command` or a codebase command.
Each reads the process's arguments, folder, environment and standard
streams directly.

```strata command_line
```

`edit` already checks only what an edit can affect. `edit_scope` compares
two versions of a package: the definitions changed, plus the users of a
changed interface. `edit_errors` checks those, with the rest of the
package in scope.

```strata edit_scope
```

# Intuition
A client and a daemon on one Unix socket. The client sends its arguments,
folder and environment, and passes its file descriptors 0, 1 and 2. The
daemon puts them in place of its own, runs `command_line` as usual, and
sends back the exit code. A terminal is still a terminal, so the bytes are
the same.

For `check`, the daemon keeps the last clean check (`CheckKept`). Say file
`driver.strata` changed and holds `hex_digit`, whose body changed. The
daemon reads that file alone. `edit_scope` says: check `hex_digit`. That
definition checks with no warning, so the output is the kept warnings and
the `ok:` line. Anything else (an error, a new warning, a hole) means a
whole check.

# The socket and the client
The C side carries the request. `daemon_forward` is the client; `daemon_next`
and `daemon_done` bracket a request in the daemon. The runtime learns the
request's arguments (`strata_host_serve`), and a closed client output no
longer exits the process.

```strata-diff daemon.client daemon.forward daemon.socket_of
```

```strata-also
daemon.command: `strata daemon start | stop | status`
daemon.root_of daemon.encoded daemon.running daemon.start daemon.listen daemon.next daemon.done: the socket and requests
daemon.idle_ms daemon.memory_limit daemon.modules daemon.resident daemon.stamp daemon.set_args daemon.served: settings and the C glue
daemon.daemon_*: the extern declarations
daemon.jit_modules daemon.log_of daemon.self_path daemon.unlisten daemon.utf8_bytes daemon.Next: helpers
```

# The daemon's loop
`daemon_serve` waits for a request, runs `command_line` with the daemon's
state (`Warm`), and gives the streams back. It stops when asked, when idle,
or when it grew too large. `command_line` sends the served commands to a
daemon when one runs. `--daemon` starts one first.

```strata-diff daemon_serve command_line Warm
```

```strata-also
main strata_command without_daemon: the state passed down; `daemon` dispatched
driver.valued_flags driver.usage_text: `--socket`, `--root`, the help
```

# Checking again only what changed
`warm_check` does a whole check as `check_command` does and keeps the parts.
`warm_again` reads the changed files, computes `edit_scope`, and checks those
definitions with `scoped_errors`. It returns `None` whenever the result could
differ from a whole check's.

```strata-diff warm_again warm_check CheckKept
```

```strata-also
check_command: goes to warm_check in a daemon
scoped_errors edit_errors: edit_errors' partial check, shared, with the library desugared reused
check_errors_in check_errors_sweet literal_errors literal_errors_sweet: the library desugared given
lint_kept same_uses: lint and unused results kept per definition
settings_error settings_error_in run_mode_errors run_mode_errors_in: the same checks on definitions in hand
outer_stamp warning_label decl_has_hole flat_decls warm_checks: helpers
```
