# Internals

## Source layout

| File | What it is |
|---|---|
| `src/main.rs`, `src/cmd/` | the command line: arguments and dispatch, then one function per command (`codebase.rs`: changing the package; `git.rs`: merge driver, sync, ci; `views.rs`; `check.rs`; `run.rs`) |
| `src/ast.rs` | the syntax tree (the canonical representation), and the walkers every pass uses (`Visit`, `VisitMut`) |
| `src/symbol.rs` | interned names: every identifier in the tree is a `Symbol`, a `u32` into one table of texts, so names compare, hash and copy as integers (`x.sym()` turns text into one) |
| `src/names.rs` | qualified names: `short`, `module_of`, `qualify`, `qualified` |
| `src/lexer.rs`, `src/parser.rs` | text → tree |
| `src/binary.rs` | the binary format, by hand: module files (a header, then each definition's interface and bodies), bundles, and the definition hashes `store` uses; see [binary-format.md](binary-format.md) |
| `src/bench.rs` | front-end timings on the examples: `cargo test --release bench -- --ignored --nocapture` |
| `src/printer.rs` | tree → text (canonical form, used for hashing) |
| `src/types.rs`, `src/types/` | type inference, traits, holes (`elaborate_typed`: the program as it runs, with each expression's inferred type, for the compiler); `env.rs` (what is declared), `exhaustive.rs` (exhaustiveness, unreachable arms), `ints.rs` / `reals.rs` (value sets of refinements) |
| `src/smt.rs`, `src/smt/regexes.rs` | SMT verification: symbolic execution of bodies into proof obligations, Z3 queries, models back to values; regexes translated to SMT-LIB |
| `src/termination.rs` | termination: call graph, loop bounds, structural and integer recursion; which helpers contracts may see through |
| `src/verify.rs` | compile-time refinement checks on literals; the layered check (prove, confirm counterexamples, fuzz the rest); contract fuzzing |
| `src/store.rs` | content-addressed codebase, dependency graph, patch log |
| `src/merge.rs` | `strata merge-file`: three-way merge per definition, renames |
| `src/package.rs` | packages: `strata.toml`, their module files (decoded, never parsed: text goes in through `edit`), finding packages and scripts |
| `src/git.rs` | running git |
| `src/llvm.rs`, `src/llvm/runtime.c` | `strata build`: the elaborated program as LLVM IR (text), and the C runtime it links with (strings and the string built-ins, printing, number formatting, faults, the system) |
| `src/llvm/prelude.strata` | the built-ins of lists, dictionaries and sets (each kept as a sorted list) and `lines`, in Strata, compiled with every program (module `strata.compiled`); the compiler calls them for `map`, `filter`, `sort`, `insert`, `union`, ... |
| `src/llvm/jit.rs`, `build.rs` | the JIT (`strata run`, `test`, `verify`, compile-time code): LLVM's libraries (inkwell) parse that IR, optimize it and run it in memory; build.rs compiles the runtime into strata |
| `src/derive.rs`, `std/typing.strata` | derived traits: each `derive Trait for Type` becomes an ordinary `impl`, its `derived fn`s specialized for the type at compile time (`comptime`, `make` / `try_make`, `f.of(x)`, `match comptime` / `if comptime`, methods called on types known while compiling, copies of functions with `comptime` parameters) |
| `std/json.strata` | JSON text and the derived `Json` trait, in Strata |
| `std/regex.strata`, `std/unicode.strata`, `std/unicode_tables.strata`, `tools/unicode_tables.py` | regular expressions in Strata: the parser and the tree (as regex-syntax's `Hir`), sets of code points, Unicode properties and case folding; the tables are generated from regex-syntax's by the script; `tests/regex.rs` compares the trees with regex-syntax's |
| `std/ast.strata`, `std/ast/binary.strata`, `std/utf8.strata`, `src/ast_value.rs` | the syntax tree in Strata, and the binary format read and written by Strata code; `ast_value` writes a Rust tree as the `std.ast` value `debug` prints (`strata header FILE --tree`), which `tests/ast.rs` compares with the Strata decoder's |
| `src/stdlib.rs`, `std/` | the standard library (Strata), built into the binary; the native `host_*` operations it alone can call |
| `src/view.rs` | level-of-detail projections and selectors |
| `src/ui.rs`, `src/ui/index.html` | `strata ui`: the live viewer (a std-only HTTP server and one page) |
| `src/serve.rs`, `src/serve/app.js` | `strata serve`: a std-only HTTP server calling the program's `handle` for each request, or running it as an app (`init` / `update` / `view`, sessions' models in compiled memory, and the page's script); the program compiled once with the server's functions (`llvm::compile_calls`, `jit::with_calls`) |
