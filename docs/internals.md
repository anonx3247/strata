# Internals

## Source layout

| File | What it is |
|---|---|
| `src/main.rs`, `src/cmd/` | the command line: arguments and dispatch, then one function per command (`codebase.rs`: changing the package; `git.rs`: merge driver, sync, ci; `views.rs`; `check.rs`; `run.rs`) |
| `src/ast.rs` | the syntax tree (the canonical representation), and the walkers every pass uses (`Visit`, `VisitMut`) |
| `src/names.rs` | qualified names: `short`, `module_of`, `qualify` |
| `src/lexer.rs`, `src/parser.rs` | text → tree |
| `src/printer.rs` | tree → text (canonical form, used for hashing) |
| `src/types.rs`, `src/types/` | type inference, traits, holes; `env.rs` (what is declared), `exhaustive.rs` (exhaustiveness, unreachable arms), `ints.rs` / `reals.rs` (value sets of refinements) |
| `src/interp.rs` | interpreter: trait dispatch, mocks, runtime contracts, JSON in/out, traces |
| `src/generate.rs` | value generators derived from types and refinements (used by mocks and fuzzing) |
| `src/smt.rs`, `src/smt/regexes.rs` | SMT verification: symbolic execution of bodies into proof obligations, Z3 queries, models back to values; regexes translated to SMT-LIB |
| `src/termination.rs` | termination: call graph, loop bounds, structural and integer recursion; which helpers contracts may see through |
| `src/verify.rs` | compile-time refinement checks on literals; the layered check (prove, confirm counterexamples, fuzz the rest); contract fuzzing |
| `src/store.rs` | content-addressed codebase, dependency graph, patch log |
| `src/merge.rs` | `strata merge-file`: three-way merge per definition, renames |
| `src/package.rs` | packages: `strata.toml`, their files, finding packages and scripts |
| `src/git.rs` | running git |
| `src/llvm.rs`, `src/llvm/runtime.c` | `strata build`: the elaborated program as LLVM IR (text), and the C runtime it links with (strings, printing, number formatting, faults) |
| `src/stdlib.rs`, `std/` | the standard library (Strata), built into the binary; the native `host_*` operations it alone can call |
| `src/view.rs` | level-of-detail projections and selectors |
| `src/ui.rs`, `src/ui/index.html` | `strata ui`: the live viewer (a std-only HTTP server and one page) |
