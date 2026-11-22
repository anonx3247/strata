# Internals

## Source layout

| File | What it is |
|---|---|
| `src/main.rs`, `src/cmd/` | the command line: arguments and dispatch, then one function per command (`codebase.rs`: changing the package; `git.rs`: merge driver, sync, upgrade; `views.rs`; `check.rs`; `run.rs`) |
| `src/ast.rs` | the syntax tree (the canonical representation), and the walkers every pass uses (`Visit`, `VisitMut`) |
| `src/symbol.rs` | interned names: every identifier in the tree is a `Symbol`, a `u32` into one table of texts, so names compare, hash and copy as integers (`x.sym()` turns text into one) |
| `src/names.rs` | qualified names: `short`, `module_of`, `qualify`, `qualified`; a fault's call stack folded and shown (`fold_stack`, `show_stack`) |
| `src/fx.rs`, `src/par.rs` | hash maps with rustc's FxHash, for small keys such as symbols; running independent work on all cores (`par::map`) |
| `src/lexer.rs`, `src/parser.rs` | text → tree |
| `src/resolve.rs` | names: the stored form (every reference to a definition by its qualified name) and the written form (as short as the imports allow), `use` and `pub`, a union's members, rename and move |
| `src/desugar.rs` | ranges, slices and comprehensions into the builtins, record defaults, unions with members of both kinds split, refinements written as type arguments given an alias |
| `src/binary.rs` | the binary format, by hand: module files (a header, then each definition's interface and bodies), bundles, and the definition hashes `store` uses; see [binary-format.md](binary-format.md) |
| `src/bench.rs` | front-end timings on the examples: `cargo test --release bench -- --ignored --nocapture` |
| `src/printer.rs` | tree → text (canonical form, used for hashing) |
| `src/types.rs`, `src/types/` | type inference, traits, holes (`elaborate_typed`: the program as it runs, with each expression's inferred type, for the compiler); `env.rs` (what is declared), `exhaustive.rs` (exhaustiveness, unreachable arms), `ints.rs` / `reals.rs` (value sets of refinements) |
| `src/smt.rs`, `src/smt/regexes.rs` | SMT verification: symbolic execution of bodies into proof obligations, Z3 queries, models back to values; regexes translated to SMT-LIB |
| `src/termination.rs` | termination: call graph, loop bounds, structural and integer recursion; which helpers contracts may see through |
| `src/verify.rs` | compile-time refinement checks on literals; the layered check (prove, confirm counterexamples, fuzz the rest); contract fuzzing |
| `src/store.rs` | content-addressed codebase, dependency graph, patch log |
| `src/pending.rs` | pending changes: an edit that breaks code elsewhere turns each broken use into a `?pending_N` hole, until `strata done` or `strata abort` |
| `src/diff.rs`, `src/history.rs` | structural diffs between two versions of a codebase; the ranges they are taken over (a package's log, or git commits plus the working tree) |
| `src/merge.rs` | `strata merge-file`: three-way merge per definition, renames |
| `src/package.rs` | packages: `strata.toml`, their module files (decoded, never parsed: text goes in through `edit`), finding packages and scripts |
| `src/git.rs` | running git |
| `src/llvm.rs`, `src/llvm/runtime.c` | the code generator: the elaborated program as LLVM IR (text), and the C runtime it links with (reference counting, big integers, the string built-ins not yet in Strata, printing, number formatting, faults, threads and channels for tasks, the system) |
| `src/llvm/prelude.strata` | the built-ins of lists, dictionaries and sets (each kept as a sorted list), `lines`, and the string built-ins written over `byte_len` / `byte_at` / `byte_slice` (`contains`, `index_of`, `split`, `replace`, …), in Strata, compiled with every program (module `strata.compiled`); the compiler calls them for `map`, `filter`, `sort`, `insert`, `union`, ... |
| `src/llvm/native.rs` | reading a value compiled code made from its memory, by its shape (compile-time values, `run f` results, counterexamples), and writing one there |
| `src/llvm/jit.rs`, `build.rs` | the JIT (`strata run`, `test`, `verify`, compile-time code): LLVM's libraries (inkwell) parse that IR, optimize it and run it in memory; build.rs compiles the runtime into strata |
| `src/derive.rs`, `std/typing.strata` | derived traits: each `derive Trait for Type` becomes an ordinary `impl`, its `derived fn`s specialized for the type at compile time (`comptime`, `make` / `try_make`, `f.of(x)`, `match comptime` / `if comptime`, methods called on types known while compiling, copies of functions with `comptime` parameters) |
| `std/json.strata` | JSON text and the derived `Json` trait, in Strata |
| `std/regex.strata`, `std/unicode.strata`, `std/unicode_tables.strata`, `tools/unicode_tables.py` | regular expressions in Strata: the parser and the tree (as regex-syntax's `Hir`), sets of code points, Unicode properties and case folding; the tables are generated from regex-syntax's by the script; `tests/regex.rs` compares the trees with regex-syntax's |
| `src/cli.rs`, `std/cli.strata` | command-line options: a record that derives `CommandLine` is read by its derived `from_args`, written in Strata; the Rust side only says which types are options |
| `std/io.strata`, `std/fs.strata`, `std/process.strata`, `std/http.strata`, `std/web.strata`, `std/code.strata` | the console, files, the process, HTTP and routes, HTML as values, and the codebase as values (`Codebase`, read by `strata ui --strata`) |
| `src/stdlib.rs`, `std/` | the standard library (Strata), built into the binary; the native `host_*` operations it alone can call |
| `src/view.rs` | level-of-detail projections and selectors |
| `src/ui.rs`, `src/ui/index.html` | `strata ui`: the workspace (a std-only HTTP server and one page) |
| `src/ui/agents.rs`, `src/ui/transcripts.rs` | its Agents mode: agent sessions over ACP, one subprocess each; the Claude Code sessions saved for the repository, read and resumed |
| `src/ui/activity.rs`, `src/ui/prs.rs` | the activity (git history by branch, commit and definition, and the structural diff between any two revisions); the PRs mode, through the local `gh` |
| `src/ui/viewer.strata` | the viewer written in Strata (`strata ui --strata`), an app served by `strata serve`'s runtime |
| `src/serve.rs`, `src/serve/app.js` | `strata serve`: a std-only HTTP server calling the program's `handle` for each request, or running it as an app (`init` / `update` / `view`, sessions' models in compiled memory, and the page's script); the program compiled once with the server's functions (`llvm::compile_calls`, `jit::with_calls`) |

## The compiler in Strata (`compiler/`)

The front end written in Strata is the `compiler/` package: an ordinary package (its modules named by their paths: `lex`, `parse`, …), a program rather than a library, and not part of std. Its `main` (`main.strata`) runs one step on files, a command each (`tree`, `same`, `hashes`, `text`, `bad`, `lex`, `parse`, `stored`, `stored-package`, `written`, `use-errors`, `check`, `eval`, `builtins`), printing what the Rust side prints; the tests build it once (`strata build compiler`, `tests/common/compiler.rs`) and compare. `check` and `package` load std lazily (`std_imports`): from the std names the definitions are written with (`binary.decl_strings`, their string tables) it follows std's header entries (each lists what its definition refers to) and decodes only the definitions reached (`binary.decode_entry`), with the impls whose type and trait are in (nothing names an impl) and, for a name that is no definition (a variant, a trait method), its module's types and traits.

| File | What it is |
|---|---|
| `ast.strata`, `binary.strata` (with `std/utf8.strata`, and `src/ast_value.rs` on the Rust side) | the syntax tree in Strata, and the binary format read and written by Strata code; `ast_value` writes a Rust tree as the `compiler/ast` value `debug` prints (`strata header FILE --tree`), which `tests/ast.rs` compares with the Strata decoder's |
| `eval.strata`, `text.strata` | the front end in Strata so far: a program run from its stored trees (its own name and method resolution, values as `compiler/ast` literals), and the printer; `tests/eval.rs` runs the programs `tests/build.rs` compiles (shared in `tests/common/programs.rs`) and compares their recorded outputs, `tests/ast.rs` the printer's text of every module file with `src/printer.rs`'s (`strata header FILE --text`); `compiler/lex.strata` is the lexer, compared with `src/lexer.rs` by `tests/lex.rs` (`strata lex FILE`), and `compiler/parse.strata` the parser, compared with `src/parser.rs` by `tests/parse.rs` (`strata parse FILE`) |
| `resolve.strata`, `check.strata` | name resolution in Strata (the stored and written forms, union members, the resolver's errors), compared with `src/resolve.rs` by `tests/resolve.rs`; the type checker in Strata so far (clashing definitions, unknown names, arity, inference for the basic types), compared with `strata check` by `tests/check.rs` |
| `lex.strata`, `parse.strata` | the lexer and the parser, compared with `src/lexer.rs` / `src/parser.rs` by `tests/lex.rs` / `tests/parse.rs` |
| `exhaustive.strata`, `builtins.strata` | non-exhaustive matches (`src/types/exhaustive.rs`), and the builtins' signatures (`src/types.rs`'s `BUILTINS`, `tests/builtins.rs`) |
