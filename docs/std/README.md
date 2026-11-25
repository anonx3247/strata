# std

The modules of `std`, each with its types, traits and functions: their signatures, contracts and docs. Generated from the code's `///` docs by `strata doc`.

| Module | |
|---|---|
| [std.cli](cli.md) | Command-line options, read into a record; commands, into a union. |
| [std.code](code.md) | A codebase as Strata's tools see it: its definitions, each rendered as text with what it uses and what uses it, the checker's findings, the verifier's cached results and the package's log. |
| [std.collections](collections.md) | A dictionary and a set kept in their keys' order, and a priority queue. |
| [std.ffi](ffi.md) | Strata functions called from other languages. |
| [std.fs](fs.md) | Files and folders. |
| [std.http](http.md) | Requests, responses and routes, for `strata serve`. |
| [std.io](io.md) | The console: text in and out. |
| [std.json](json.md) | JSON text, and records and sum types read from it and written to it. |
| [std.memory](memory.md) | Memory, for programs that must be fast: arenas. |
| [std.process](process.md) | The running program: its arguments, its environment variables and its exit code, and the other programs it runs. |
| [std.regex](regex.md) |  |
| [std.term](term.md) | Text styled for a terminal: bold, dim, colors. |
| [std.time](time.md) | The clock: the time now, waiting, and timers in `select`. |
| [std.toml](toml.md) | TOML text (version 1.1, https://toml.io/en/v1.1.0) read into values and written from them, and records and sum types read from TOML and written to it. |
| [std.typing](typing.md) | Types as values, for derivable traits. |
| [std.unicode](unicode.md) | Sets of code points, Unicode's properties and simple case folding (Unicode 16.0.0, from std.unicode_tables). |
| [std.unicode_tables](unicode_tables.md) | Unicode 16.0.0 character data, for `std.unicode`. |
| [std.utf8](utf8.md) | Text as UTF-8 bytes, and back. |
| [std.web](web.md) | HTML pages, built as values. |
