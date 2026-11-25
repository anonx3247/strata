# std

The modules of `std`, each with its types, traits and functions: their signatures, contracts and docs. Generated from the code's `///` docs by `strata doc`.

| Module | |
|---|---|
| [std.cli](cli.md) | std.cli: command-line options read into a record, and commands into a union, with `derive CommandLine`. |
| [std.code](code.md) | std.code: a codebase as Strata's tools see it, for viewers and agents. |
| [std.collections](collections.md) | std.collections: ordered dictionaries and sets (AVL trees), and a priority queue (a binary heap). |
| [std.ffi](ffi.md) | std.ffi: Strata functions called from other languages. |
| [std.fs](fs.md) | std.fs: files and folders. |
| [std.http](http.md) | std.http: requests, responses and routes for `strata serve`. |
| [std.io](io.md) | std.io: the console, text in and out. |
| [std.json](json.md) | std.json: JSON text, and records and sum types read from it and written to it. |
| [std.memory](memory.md) | std.memory: arenas, for programs that allocate much and keep little. |
| [std.orm](orm.md) | std.orm: records as SQLite tables. |
| [std.process](process.md) | std.process: the running program's arguments, environment and exit code, and the programs it runs. |
| [std.python](python.md) | std.python: calls to Python functions declared `extern python "module" fn …`. |
| [std.regex](regex.md) | std.regex: regular expressions in regex-syntax's syntax, matched by a Pike VM or a DFA. |
| [std.sqlite](sqlite.md) | std.sqlite: SQLite databases through the system's libsqlite3. |
| [std.term](term.md) | std.term: text styled for a terminal with ANSI escape codes. |
| [std.time](time.md) | std.time: the clock, waiting, and timers in `select`. |
| [std.toml](toml.md) | std.toml: TOML 1.1 text (https://toml.io/en/v1.1.0) read into values and written from them, and `derive Toml` for records and sum types. |
| [std.typing](typing.md) | std.typing: types as values at compile time, for derivable traits. |
| [std.unicode](unicode.md) | std.unicode: sets of code points, Unicode properties, simple case folding and grapheme clusters (Unicode 16.0.0). |
| [std.unicode_tables](unicode_tables.md) | std.unicode_tables: Unicode 16.0.0 character data for `std.unicode`. |
| [std.utf8](utf8.md) | std.utf8: text as UTF-8 bytes, and back. |
| [std.web](web.md) | std.web: HTML pages built as values, one function per element, rendered by `render` or `document`. |
