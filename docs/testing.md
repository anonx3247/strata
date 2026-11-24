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
