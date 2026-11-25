# Testing

Strata is tested in three ways:

- **Proofs.** Contracts, refinements and laws on the Strata code are proven by `strata verify`. std must stay proven. For a changed module, run `strata --file std verify 'std.MODULE.*' --require proven`. Verifying all of std takes over 20 minutes, so verify only what changed, by name.
- **`test` blocks** in the Strata packages (`strata --file std test`, `strata --file compiler test`, `strata --file core test`), for pure code that isn't proven yet.
- **Golden-output cases** in `tests/cases`, run by `tools/check` against a `strata` executable. They cover what depends on the outside world: files, processes, git, editors, command-line output and built executables.

## Running the cases

```
strata run tools/check -- --strata target/strata                    # every case
strata run tools/check -- --strata ./strata build- cli-verify        # cases whose names contain these words
strata run tools/check -- --strata ./strata --bless codegen-programs # rewrite their golden files
```

Cases run 4 at a time, each in its own task. `--jobs N` changes the number, and `--jobs 1` runs them one at a time. `--keep` keeps a failed case's folder.

## Writing a case

A case is a file `tests/cases/NAME.toml` holding a list of `[[step]]`s, run in order. Each case runs in a fresh folder, `$TMPDIR/strata-check.XXXXXX/NAME`, created by `mktemp -d` so that concurrent runs never share one. The folder is removed when the case passes. The fields of a step are those of `tools/check`'s `Step` record.

A step does one of these:

- stores a module: `save = "x.strata"` with `text`, or `from = "x.st"` for a file under `tests/cases/NAME/`. `module` and `into` target a module of a folder or package.
- writes a file (`write`)
- runs `strata` (`strata = [...]`) or another program (`run = [...]`)

It can expect:

- an exit code: `code`, `fails` or `any_code`. The default is 0.
- text in the output: `has`, `lacks`, `counts`
- the whole output: `out`, `stdout`, `out_file` (standard output), or `golden`. A golden file lives in the repository and holds standard output, then standard error, then `--- exit N`, in the format of `tests/data/build/expected`.

Other fields:

- `needs = "python3"` skips the rest of the case, which then passes, when that program isn't on the `PATH`.
- `memory = true` runs a built program with `STRATA_MEMORY=check`, which fails unless the program frees everything it allocated.

Variables: `$ROOT` is the repository, `$TMP` the case's folder, and `$STRATA` (in `run`) the executable under test.

Each command keeps its `verify` verdicts in the folder it runs in (`STRATA_VERDICTS`). For a deterministic summary, `strata run --verify-alongside` with `STRATA_SWAP_WAIT=start` starts the program only after its verifier has finished and the proven code is swapped in (see `cli-run_verifies_alongside`).

`build-parallel_io` checks that concurrent `std.process` runs don't leak pipes into each other's programs, and that concurrent `std.fs` calls don't share a path buffer.

## Reproducible builds

`tools/reproducible.sh [PACKAGE] [FLAGS…]` builds a package twice and checks that the two executables are byte-identical. The defaults are `compiler --no-verify --release`, with `$STRATA` or `target/strata`. The first build starts from an empty cache. The second keeps only the package's compile-time values, std's library and core.rt. This checks that what a process ran before compiling, such as the JIT evaluating compile-time values, doesn't change the objects it produces. It takes about a minute for `compiler/`. Run it after changing the toolchain (`compiler/c/`). The bootstrap's fixed point (`tools/bootstrap.sh`) depends on it.

## Recorded corpora

Some cases replay outputs recorded from a reference implementation:

- `regex-curated`: std.regex's trees, relations, samples and SMT models of curated patterns (`tests/data/regex-curated.out`), recorded while they agreed with the `regex` crates.
- `toml-documents`: std.toml's readings of 104 documents, recorded while they agreed with the `toml` crate.
- `check-recorded_errors`: the errors `strata check` gives for 41 programs.
- `smt-recorded_verdicts`: `strata verify` on each program in `tests/data/smt`.
- `codegen-programs-1` … `-5` and `codegen-constructs`: programs built by `strata build`, against golden outputs in `tests/data/build/expected/codegen-*.out`.

The last Rust implementation, and the cargo tests that compared it with the Strata one, are at the tag `last-rust`.
