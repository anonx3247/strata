# std

The modules of `std`, each with its types, traits and functions: their signatures, contracts and docs. Generated from the code's `///` docs by `strata docgen`.

| Module | |
|---|---|
| [std.agent](agent.md) | An LLM agent loop: the model calls tools, the loop runs them and answers, until a reply calls none. |
| [std.array](array.md) | N-dimensional arrays of floats and integers, stored flat with a shape and strides. |
| [std.cli](cli.md) | Command-line options read into a record, and commands into a union, with `derive CommandLine`. |
| [std.code](code.md) | A codebase as Strata's tools see it, for viewers and agents. |
| [std.collections](collections.md) | Ordered dictionaries and sets (AVL trees), and a priority queue (a binary heap). |
| [std.ffi](ffi.md) | Strata functions called from other languages. |
| [std.fs](fs.md) | Files and folders. |
| [std.http](http.md) | HTTP/1.1 requests and responses, a client (`send`, `fetch`, TLS for `https://`) and a server (`serve`, a `Router`), over any `std.net.Stream`. |
| [std.io](io.md) | The console, text in and out. |
| [std.json](json.md) | JSON text, and records and sum types read from it and written to it. |
| [std.lemmas](lemmas.md) | Facts about the builtin collections, proven once by `verify`, for proofs to use (`assert … by frame_with(d, k, v, done)`). |
| [std.memory](memory.md) | Arenas, for programs that allocate much and keep little. |
| [std.net](net.md) | TCP connections and listeners, through the `Net` capability. |
| [std.orm](orm.md) | Records as SQLite tables: `derive Table for Todo` gives the table's schema, its rows written and read back as `Todo`s, and typed queries over its fields. |
| [std.process](process.md) | The running program's arguments, environment and exit code, and the programs it runs. |
| [std.python](python.md) | Calls to Python functions declared `extern python "module" fn …`. |
| [std.regex](regex.md) | Regular expressions in regex-syntax's syntax, matched by a Pike VM or a DFA. |
| [std.rust](rust.md) | Calls to Rust functions declared `extern rust "crate" fn …`. |
| [std.sqlite](sqlite.md) | SQLite databases through the system's libsqlite3. |
| [std.term](term.md) | Text styled for a terminal with ANSI escape codes. |
| [std.time](time.md) | The clock, waiting, and timers in `select`. |
| [std.toml](toml.md) | TOML 1.1 text (https://toml.io/en/v1.1.0) read into values and written from them, and `derive Toml` for records and sum types. |
| [std.ts](ts.md) | Calls to TypeScript functions declared `extern ts "module" fn …`. |
| [std.typing](typing.md) | Types as values at compile time, for derivable traits. |
| [std.unicode](unicode.md) | Sets of code points, Unicode properties, simple case folding and grapheme clusters (Unicode 16.0.0). |
| [std.unicode_tables](unicode_tables.md) | Unicode 16.0.0 character data for `std.unicode`. |
| [std.utf8](utf8.md) | Text as UTF-8 bytes, and back. |
| [std.web](web.md) | HTML pages built as values, one function per element, rendered by `render` or `document`. |
