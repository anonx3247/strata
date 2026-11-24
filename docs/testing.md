# Testing without Rust

The cargo tests (`tests/*.rs`, the `#[test]`s in `src/`) leave with the Rust
implementation. What they guard moves to three places:

- **Proofs**: contracts, refinements and laws on the Strata code, proven by
  `strata verify` (std must stay proven: `strata --file std verify
  'std.MODULE.*' --require proven` for a changed module).
- **`test` blocks** in the Strata packages (`strata --file std test`,
  `strata --file compiler test`, `strata --file core test`), for pure code
  that isn't proven yet.
- **Golden-output cases** (`tests/cases`), run by `tools/check` against a
  `strata` executable: what depends on the outside world (files, processes,
  git, editors, the command line's output, built executables).

## Running the cases

```
strata run tools/check -- --strata target/release/strata            # every case
strata run tools/check -- --strata ./strata build- cli-verify        # cases whose names hold these
strata run tools/check -- --strata ./strata --bless codegen-programs # rewrite their golden files
```

A case is `tests/cases/NAME.toml`, a list of `[[step]]`s run in order in a
fresh folder (`$TMPDIR/strata-check.XXXXXX/NAME`: the run's own folder, made by `mktemp -d`, so runs side by side never share one; removed when the case passes;
`--keep` keeps it). A step stores a module (`save = "x.strata"` with `text`,
or `from = "x.st"` under `tests/cases/NAME/`; `module`/`into` for a module of
a folder or package), writes a file (`write`), runs `strata` (`strata =
[...]`) or another program (`run = [...]`), and expects an exit code (`code`,
`fails`, `any_code`; 0 by default), texts in the output (`has`, `lacks`,
`counts`), or the whole output (`out`, `stdout`, `out_file` (standard output), or `golden`: a
file under the repository, standard output then standard error and `---
exit N`, as `tests/data/build/expected` holds them). `memory = true` runs a
built program with `STRATA_MEMORY=check`: it must free everything it made.
`$ROOT` is the repository, `$TMP` the case's folder, `$STRATA` (in `run`) the
executable checked. Each command's `verify` verdicts are kept in the folder it
runs in (`STRATA_VERDICTS`). The steps are `tools/check`'s `Step` record.

Cases run 4 at a time (`--jobs N`; `--jobs 1` for one at a time), each in a
task of its own. Parallel runs once hung and removed files of `tests/cases`:
the runtime's `std.process` runs leaked their pipes into each other's
programs, and its `std.fs` calls shared one buffer for the path (fixed in
runtime.c; `build-parallel_io` checks both).

## Where each cargo test goes

Category (a): compares the Rust implementation with the Strata one; leaves
with Rust. (b): behaviour of `strata` and of built programs; a golden-output
case. (c): a property of pure code; a proof, a `test` block, or a recorded
corpus run as a case.

| Test file | Tests | Cat. | Where it goes |
|---|---|---|---|
| cli.rs (155) | every test but the 9 below | b | `tests/cases/cli-TEST.toml`, one case per test |
| cli.rs | `std_is_proven` | c | `strata --file std verify 'std.MODULE.*' --require proven` per changed module (whole std is 20+ min) |
| cli.rs | `std_termination_is_kept_between_runs` | b | not ported: reads Rust's cache files (`termination/`); the content-hash cache has its own test (#377) |
| cli.rs | `serve_calls_handle_for_each_request`, `serve_app_capabilities_timers_and_keys`, `serve_runs_an_app`, `serve_apps_take_values_and_forms`, `ui_in_strata_shows_the_code`, `ui_workspace_activity_agents_and_prs`, `ui_page_defines_each_name_once` | b | dropped with `ui`/`serve` (out of v0.1) |
| build.rs (41) | every test | b | `tests/cases/build-*.toml` (golden: `tests/data/build/expected`, 6 new `iterators-*.out`), `build-options`, `build-jit*`, `build-emit`, `build-unsupported`, `build-no_limits`, `build-extern_c`, `build-io-cancel` |
| build.rs | `reads_are_cancelled`, its `blocked` program | b | not ported: needs standard input held open (a pipe); the harness's `run_with` closes it |
| codegen.rs | `compiled_programs_do_what_rust_compiled_ones_do`, `compiler_constructs_do_what_rust_compiled_ones_do` | a→b | `codegen-programs-1` … `-5`, `codegen-constructs`: built by the default `strata build`, golden outputs recorded from the llvm.rs builds (`tests/data/build/expected/codegen-*.out`) |
| codegen.rs | `build_compare_agrees_on_the_programs_and_examples`, `jit_runs_programs_as_built_ones_run`, `jit_traces_as_rust_traces`, `tests_run_as_strata_test_runs_them`, `mocks_as_strata_test_runs_them`, `fuzzing_as_verify_fuzzes`, `compiler_fuzzes_as_verify_fuzzes`, `foreign_calls_do_what_rust_compiled_ones_do`, `core_rt_*_as_in_c` (5), `core_rt_compiled_by_codegen` | a | leave with Rust (`--compare`, llvm.rs, runtime.c side by side); the build, run and test cases cover the Strata side |
| codegen.rs | `build_and_test_use_compiler_codegen_by_default`, `refuses_what_it_does_not_compile_yet`, `leaves_out_what_verify_proved`, `package_build_leaves_out_what_verify_proved`, `a_let_of_a_union_with_refined_members_is_checked`, `distinct_members_of_unions_are_checked_on_their_field`, `jit_keeps_compile_time_values_between_runs`, `jit_uses_the_std_library_not_the_compilers_copies`, `std_precompiled_once_links_with_programs`, `core_rt_that_doesnt_check_isnt_compiled`, `compiler_package_compiles_with_codegen` | b | to port (they name `compiler` subcommands directly: `codegen --proofs`, `jit`, `--library`; port once agent A's CLI settles their names); `cli-proven_checks_are_left_out_of_builds` covers proofs in builds |
| eval.rs | `programs_run_as_compiled` | b | `compiler/eval` on the build programs: the `build-*` goldens hold the outputs; a case once the compiled `strata` has an eval command |
| ast.rs | `bad_input_is_an_error_not_a_crash` | b | `compiler-bad_input.toml` |
| ast.rs | `sha256_agrees_with_the_sha2_crate` | c | `test` blocks in `compiler/binary`: FIPS 180's examples, and each padding boundary (55, 56, 63, 64, 65, 119, 120, 128 bytes) |
| ast.rs | `*_as_in_rust` (4) | a | leave with Rust; `compiler/binary`'s round trips are `cli-render_round_trips`, `cli-every_example_round_trips_through_render`, `cli-pack_a_package_into_one_file` |
| regex.rs | `curated_*` (5) | c | `regex-curated.toml`: std.regex's trees, relations, samples and SMT models of the curated patterns, recorded (`tests/data/regex-curated.out`) while they agreed with the `regex` crates |
| regex.rs | `random_*` (5) | a | leave with Rust (random patterns against the crates); `samples` matching its pattern is a law to prove in std.regex (to do) |
| toml.rs | `reads_toml_as_the_toml_crate_does` | c | `toml-documents.toml`: the 104 documents, recorded while std.toml agreed with the `toml` crate |
| toml.rs | `errors_say_where`, `derives_toml` | c | `toml-errors-and-derive.toml` |
| lex.rs, parse.rs, builtins.rs, compare.rs (7), check.rs (39 `*_as_in_rust` + `compare_flag_agrees_on_a_script`), codebase.rs (5), resolve.rs (`stores_as_in_rust`, `use_errors_as_in_rust`) | | a | leave with Rust: they compare the two front ends. The Strata side is what the `cli-*` cases run (`check`, `edit`, `render`, the codebase commands) |
| resolve.rs | `written_as_rendered`, `path_dependencies`, `dependency_rules`, `git_dependencies` | b | to port (dependencies through `compiler package`; port with agent A's `strata` package commands) |
| smt.rs | `*_as_in_rust` (8) | a | leave with Rust; the verify cases (`cli-smt_*`, `cli-*verify*`, `cli-termination_*`, ...) check the Strata verifier's verdicts |
| smt.rs | `proofs_and_statuses_follow_the_code`, `verify_fuzzes_around_what_cant_be_compiled`, `codegen_names_every_driver_that_cant_be_compiled`, `verify_doesnt_fuzz_what_holds_a_capability`, `smt_shares_lists_both_branches_drop_from`, `smt_valid_lists_shared_by_both_branches`, `verify_doesnt_run_raw_and_finds_absolute_native_paths` | b | to port (they run `compiler verify` / `compiler proofs` directly) |
| src/ (21 unit tests: binary, smt, resolve, printer, diff, symbol, core, color, ast_value, sized, llvm/native) | | a | leave with Rust: they test Rust internals |

## Found while porting

Against `target/release/strata` built from main (aa07a2c), with
`STRATA_BACKEND=rust` (what the cargo tests run):

- `cli-dict_and_set_literals`, `cli-ranges_slices_and_comprehensions` fail,
  as their cargo tests do on main: `strata --file BUNDLE render` says "a
  bundle, not a module file".
- `cli-own_methods_locals_and_function_values` fails, as its cargo test does:
  `k.bump()` on a `mut self` method now needs `mut k.bump()`.
- `loops_chains_and_80_columns` counted long lines in the output of `render`
  on a package, which is an error message: it checked nothing (left out).

Against the same executable with its defaults (compiler/codegen builds, the
Strata verifier and JIT), 196 cases: 143 pass, 51 fail, and 2 run past the
300 s limit (`cli-comptime_in_ordinary_code_runs_while_compiling`,
`cli-long_list_literals_compile_in_linear_time`). Among the failures:
programs that `run` prints nothing for (`cli-comparing_with_a_bare_constructor_is_a_tag_test`,
`cli-command_line_options_are_read_into_a_record`, `cli-commands_run_programs`,
`cli-run_a_binary_file`), built programs that leak or crash
(`build-clock`: 1 object left; `build-capabilities`: signal 11), field
refinements not checked on assignment (`build-data`, `build-dyn`,
`cli-procedural_mutation_is_explicit`), a run that crashes
(`cli-self_arguments_on_trait_values`: signal 10), fault messages that name
`Graphemes.at` without its module, test names without their module
(`✓ a block holds…` for `✓ core.mem: a block holds…`), the Strata verifier's solver flags and missing
summary line (`cli-solver_flags_follow_the_solver`,
`cli-example_checks_tests_and_verifies`).
