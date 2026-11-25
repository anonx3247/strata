# Testing

Strata is tested in three places:

- **Proofs**: contracts, refinements and laws on the Strata code, proven by
  `strata verify` (std must stay proven: `strata --file std verify
  'std.MODULE.*' --require proven` for a changed module; whole std takes
  20+ minutes, so verify what changed by name).
- **`test` blocks** in the Strata packages (`strata --file std test`,
  `strata --file compiler test`, `strata --file core test`), for pure code
  that isn't proven yet.
- **Golden-output cases** (`tests/cases`), run by `tools/check` against a
  `strata` executable: what depends on the outside world (files, processes,
  git, editors, the command line's output, built executables).

(Until v0.1 the compiler was also written in Rust, and cargo tests compared
the two; they left with it, at the tag `last-rust`.)

## Running the cases

```
strata run tools/check -- --strata target/strata                    # every case
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
exit N`, as `tests/data/build/expected` holds them). `needs = "python3"`
skips the rest of the case (which passes) when that program isn't on the
`PATH`. `memory = true` runs a
built program with `STRATA_MEMORY=check`: it must free everything it made.
`$ROOT` is the repository, `$TMP` the case's folder, `$STRATA` (in `run`) the
executable checked. Each command's `verify` verdicts are kept in the folder it
runs in (`STRATA_VERDICTS`). The steps are `tools/check`'s `Step` record.

Cases run 4 at a time (`--jobs N`; `--jobs 1` for one at a time), each in a
task of its own. Parallel runs once hung and removed files of `tests/cases`:
the runtime's `std.process` runs leaked their pipes into each other's
programs, and its `std.fs` calls shared one buffer for the path (fixed in
runtime.c; `build-parallel_io` checks both).

## Reproducible builds

`tools/reproducible.sh [PACKAGE] [FLAGS…]` (default `compiler --no-verify
--release`, with `$STRATA` or `target/strata`) builds a package with an
empty cache, then again with only its compile-time values, std's library and
core.rt kept, and checks the two executables are the same bytes: what a
process ran before compiling (the JIT, for compile-time values) must not
change the objects it makes. About a minute for compiler/; run it with a
change to the toolchain (compiler/c/). The bootstrap's fixed point
(`tools/bootstrap.sh`) depends on it.

## Where the cargo tests went

Category (a): compares the Rust implementation with the Strata one; leaves
with Rust. (b): behaviour of `strata` and of built programs; a golden-output
case. (c): a property of pure code; a proof, a `test` block, or a recorded
corpus run as a case.

| Test file | Tests | Cat. | Where it goes |
|---|---|---|---|
| cli.rs (155) | every test but the 9 below | b | ported → `tests/cases/cli-TEST.toml`, one case per test |
| cli.rs | `std_is_proven` | c | ported → proof: `strata --file std verify 'std.MODULE.*' --require proven` per changed module (whole std is 20+ min) |
| cli.rs | `std_termination_is_kept_between_runs` | b | ported → case `cli-std_verdicts_are_kept_between_runs`: rewritten against compiler/cache's keys (the verdicts of the std functions a program reaches, kept under `verify-strata/`, all read back by a second run, which says the same) |
| cli.rs | `serve_calls_handle_for_each_request`, `serve_app_capabilities_timers_and_keys`, `serve_runs_an_app`, `serve_apps_take_values_and_forms`, `ui_in_strata_shows_the_code`, `ui_workspace_activity_agents_and_prs`, `ui_page_defines_each_name_once` | b | dropped: `ui`/`serve` are out of v0.1 |
| build.rs (41) | every test | b | ported → `tests/cases/build-*.toml` (golden: `tests/data/build/expected`, 6 new `iterators-*.out`), `build-options`, `build-jit*`, `build-emit`, `build-unsupported`, `build-no_limits`, `build-extern_c` |
| build.rs | `reads_are_cancelled` | b | ported → case `build-io-cancel`: `lines` (the input ends) and `blocked` (standard input held open through a fifo, nothing written: the reader loses 20 of 20 races, the sibling's panic is contained), built and run, everything freed |
| codegen.rs | `compiled_programs_do_what_rust_compiled_ones_do`, `compiler_constructs_do_what_rust_compiled_ones_do` | a→b | ported → `codegen-programs-1` … `-5`, `codegen-constructs`: built by the default `strata build`, golden outputs recorded from the llvm.rs builds (`tests/data/build/expected/codegen-*.out`) |
| codegen.rs | `build_compare_agrees_on_the_programs_and_examples` | a | dropped: `--compare` builds with llvm.rs too. Its programs' outputs are the `codegen-programs-*` and `build-*` goldens |
| codegen.rs | `jit_runs_programs_as_built_ones_run`, `jit_traces_as_rust_traces` | a | dropped: they compare with llvm.rs. The JIT side is `build-jit`, `build-jit-default`, `cli-run_mode_console_in_tests_and_trace` (`--trace`) |
| codegen.rs | `tests_run_as_strata_test_runs_them`, `mocks_as_strata_test_runs_them` | a | dropped: they compare with llvm.rs's `strata test`. Covered by `build-jit-tests`, `build-mock`, `cli-mocks_for_tests`, `codegen-default_build_and_test` |
| codegen.rs | `fuzzing_as_verify_fuzzes`, `compiler_fuzzes_as_verify_fuzzes` | a | dropped: they compare with Rust's fuzzer. Covered by `build-jit-verify`, `cli-fuzzes_strings_a_refinement_pattern_matches`, `cli-generic_static_methods_are_fuzzed`, `smt-recorded_verdicts` |
| codegen.rs | `foreign_calls_do_what_rust_compiled_ones_do` | a | dropped: compares with llvm.rs. Covered by `build-extern_c`, `cli-extern_c_declarations` |
| codegen.rs | `core_rt_*_as_in_c` (5), `core_rt_compiled_by_codegen` | a | dropped: they compare core.rt with runtime.c, which leaves with Rust. Covered by `cli-core_rt_is_compiled_for_the_c_runtime`, `codegen-core_rt_that_doesnt_check`, `core`'s `test` blocks (`strata --file core test`) and every built program (`build-*` with `memory = true`) |
| codegen.rs | `build_and_test_use_compiler_codegen_by_default` | b | ported → case `codegen-default_build_and_test` |
| codegen.rs | `refuses_what_it_does_not_compile_yet` | b | ported → case `codegen-refuses_what_it_does_not_compile` |
| codegen.rs | `leaves_out_what_verify_proved`, `package_build_leaves_out_what_verify_proved` | b | ported → cases `codegen-leaves_out_what_verify_proved`, `codegen-package_build_leaves_out_proven_checks` (and `cli-proven_checks_are_left_out_of_builds`) |
| codegen.rs | `a_let_of_a_union_with_refined_members_is_checked`, `distinct_members_of_unions_are_checked_on_their_field` | b | ported → cases `codegen-a_let_of_a_union_with_refined_members_is_checked`, `codegen-distinct_members_of_unions_are_checked` |
| codegen.rs | `jit_keeps_compile_time_values_between_runs` | b | ported → case `codegen-comptime_values_kept_between_runs` |
| codegen.rs | `jit_uses_the_std_library_not_the_compilers_copies`, `std_precompiled_once_links_with_programs` | b | ported → cases `codegen-std_library_in_runs`, `codegen-std_library_precompiled` |
| codegen.rs | `core_rt_that_doesnt_check_isnt_compiled` | b | ported → case `codegen-core_rt_that_doesnt_check` (`strata --file core/ edit gen` now edits core's module `core.gen`, in core's file `gen.strata`) |
| codegen.rs | `compiler_package_compiles_with_codegen` | b | ported → case `codegen-compiler_package_compiles` |
| eval.rs | `programs_run_as_compiled` | b | ported → case `eval-programs_run_as_compiled`: `strata eval` on the build programs and the `build-*` cases' programs (50), against their goldens; not `capabilities-files_and_environment` (compile-time code has no files or arguments) |
| ast.rs | `bad_input_is_an_error_not_a_crash` | b | ported → case `compiler-bad_input` |
| ast.rs | `sha256_agrees_with_the_sha2_crate` | c | ported → `test` blocks in `compiler/binary`: FIPS 180's examples, and each padding boundary (55, 56, 63, 64, 65, 119, 120, 128 bytes) |
| ast.rs | `*_as_in_rust` (4) | a | dropped: they compare the two encoders. `compiler/binary`'s round trips are `cli-render_round_trips`, `cli-every_example_round_trips_through_render`, `cli-pack_a_package_into_one_file` |
| regex.rs | `curated_*` (5) | c | ported → case `regex-curated`: std.regex's trees, relations, samples and SMT models of the curated patterns, recorded (`tests/data/regex-curated.out`) while they agreed with the `regex` crates |
| regex.rs | `random_*` (5) | a | dropped: random patterns against the `regex` crates. `regex-curated` and `build-regex` cover std.regex; `samples` matching its pattern is a law to prove in std.regex (to do) |
| toml.rs | `reads_toml_as_the_toml_crate_does` | c | ported → case `toml-documents`: the 104 documents, recorded while std.toml agreed with the `toml` crate |
| toml.rs | `errors_say_where`, `derives_toml` | c | ported → case `toml-errors-and-derive` |
| lex.rs, parse.rs, resolve.rs `stores_as_in_rust` | | a | dropped: they compare the two front ends' tokens, trees and stored modules. Every module file of the repository going through the Strata lexer, parser and encoder is `cli-every_example_round_trips_through_render`, `cli-render_round_trips`, and the bootstrap (the compiler package compiling itself) |
| builtins.rs | `builtins_as_in_rust` | a | dropped: compares the two `builtins` listings. Covered by `cli-core_declares_the_builtins` |
| compare.rs (7) | | a | dropped: `--compare` runs llvm.rs and the Rust front end beside the Strata one |
| check.rs | 39 `*_as_in_rust`, `derives_as_in_rust` | a | ported → case `check-recorded_errors`: each program's errors from `strata check`, recorded while the Strata checker gave exactly Rust's (all 41 programs agreed, and `same_name_in_two_modules`) |
| check.rs | `compare_flag_agrees_on_a_script` | a | dropped: `--compare` leaves with Rust |
| resolve.rs | `use_errors_as_in_rust` | a | ported → `check-recorded_errors` (its last steps: the errors of a `use` of a missing module, a missing or private definition, a mock, a foreign function, a module not used, recorded while they were Rust's) |
| resolve.rs | `written_as_rendered` | b | dropped: `strata render` is now `compiler/resolve`'s `written` and `compiler/text`, the code it compared against; covered by `cli-render_round_trips`, `cli-every_example_round_trips_through_render`, `cli-unions_render_by_their_names` |
| resolve.rs | `path_dependencies`, `dependency_rules`, `git_dependencies` | b | ported → cases `resolve-path_dependencies`, `resolve-dependency_rules`, `resolve-git_dependencies` |
| codebase.rs (5) | | a | dropped: they compare the Rust and Strata codebase commands. The Strata side is the `cli-*` cases of `init`, `log`, `edit`, `rename`, `move`, `diff`, `sync`, `upgrade`, `merge-file`, pending changes and holes (`cli-changes_are_committed_to_git`, `cli-sync_*`, `cli-merge_file_*`, `cli-a_pending_change_*`, …) |
| smt.rs | `smt_outcomes_as_in_rust`, `termination_as_in_rust`, `verify_as_in_rust` | a | ported → case `smt-recorded_verdicts`: `strata verify` on each of `tests/data/smt`, recorded while the verdicts were Rust's (but `verify_byte_at`, where Rust's prover had a bug, `verify_capability`, where Rust fuzzed or proved what the Strata verifier skips, and `termination`'s `ack`, stopped by the step limit where Rust's overflowed its stack) |
| smt.rs | `smt_outcomes_on_scripts_as_in_rust`, `smt_outcomes_on_std_as_in_rust`, `verify_chooses_checks_and_rules_as_in_rust`, `verify_traits_example_as_in_rust`, `verify_solver_inputs_graphemes_and_queries_as_in_rust` | a | dropped: they compare the two verifiers. The Strata verifier's verdicts are `smt-recorded_verdicts`, `cli-smt_*`, `cli-*verify*`, `cli-termination_*`, `cli-laws_are_proven_by_the_solver`, `cli-traits_laws_and_distinct_types`, `cli-package_verify_policies`, and std staying proven (`--require proven`) |
| smt.rs | `verify_fuzzes_generic_functions_as_in_rust` | a | dropped: compares with Rust's fuzzer; covered by `cli-generic_static_methods_are_fuzzed` |
| smt.rs | `proofs_and_statuses_follow_the_code` | b | ported → case `smt-proofs_and_statuses_follow_the_code` |
| smt.rs | `verify_fuzzes_around_what_cant_be_compiled`, `codegen_names_every_driver_that_cant_be_compiled` | b | ported → cases `smt-verify_fuzzes_around_what_cant_be_compiled`, `smt-codegen_names_every_driver_that_cant_be_compiled` (the drivers named, not the reasons, which change as codegen grows) |
| smt.rs | `verify_doesnt_fuzz_what_holds_a_capability` | b | ported → case `smt-verify_doesnt_fuzz_what_holds_a_capability` |
| smt.rs | `smt_shares_lists_both_branches_drop_from`, `smt_valid_lists_shared_by_both_branches` | b | ported → cases `smt-shares_lists_both_branches_drop_from`, `smt-valid_lists_shared_by_both_branches` (10 s each, under `perl alarm`) |
| smt.rs | `verify_doesnt_run_raw_and_finds_absolute_native_paths` | b | ported → case `smt-verify_doesnt_run_raw_and_finds_native_paths` |
| src/ (21 unit tests: binary, smt, resolve, printer, diff, symbol, core, color, ast_value, sized, llvm/native) | | a | dropped: they test Rust internals |
