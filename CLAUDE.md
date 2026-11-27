# Working on Strata

Where things are tracked:
- [BUGS.md](BUGS.md): bugs to fix. A PR that fixes one removes its entry.
- [ISSUES.md](ISSUES.md): larger design problems.
- [REBUILD.md](REBUILD.md): the plan for rebuilding the language, its rules and the decisions in force.
- [IDEAS.md](IDEAS.md): ideas for possible future changes, nothing else.
- The README and `docs/` describe what is built.

## Comments and docs

- A comment never states a fact about the code ("this holds for all X", "every caller does Y", "we guarantee Z"). Write it as a contract, invariant, law or lemma, where it is checked.
- Doc comments: a line or a few on each module, type and function, saying what it is for. Elsewhere, a comment only to name or sketch an algorithm; prefer naming the function after it.
- If a comment explains what something is, rename the thing instead.

## Writing code

- Specification first: types, signatures with contracts, laws and invariants, with `?hole` bodies; `strata check` until they fit; then fill each hole (`strata holes` gives its goal and context; a smaller model can fill one) and `verify`.
- Prove instead of testing: properties as contracts, laws and lemmas proven by `verify`; tests only for what touches the outside world.

Strata is written in Strata: the `strata` command is the `compiler/` package, built by a `strata` before it. There is no Rust any more (the last Rust implementation is the tag `last-rust`).

## Conventions

- Commit each step (the user wants every change tracked) and push to `origin main` (github.com/anonx3247/strata).
- Build: `tools/bootstrap.sh` builds `target/strata` with itself (a fixed point: stage 1 is the seed's build, each next stage the previous one's, until two in a row are the same bytes; 2 builds from a seed of the same sources, up to 4 from an older one such as the last release). The seed is `STRATA_SEED`, else a `strata` on the PATH, else the latest release (downloaded). It needs LLVM 23, zstd and clang (`brew install llvm zstd`) and lld's libraries (`tools/build-lld.sh`, into `target/lld`). While iterating, run the compiler from source in the JIT (`strata run compiler -- …`) or build one stage: `STRATA_HOME=$PWD target/strata build compiler --no-verify --release -o target/dev/strata` (about 80 s), then test with `target/dev/strata`. A PR needs one stage built from main's `target/strata` plus its cases; run the full bootstrap only for changes to codegen, the runtime or core, and before a release.
- There is no CI (user's call): before every push, run the cases for what you changed and `strata verify` the functions you changed by name (`strata --file compiler verify 'codegen.lower'`; never a whole package for a partial change). std must stay proven (its precompiled library leaves out the checks inside its functions): for a changed std function, `strata --file std verify 'std.module.fn' --require proven`.
- Tests: the golden-output cases in `tests/cases`, run by `tools/check` (docs/testing.md): `strata run tools/check -- --strata target/strata NAME…` runs the cases whose names hold those words (`build-`, `cli-verify`, `codegen-`), `--jobs N` (default 4), `--keep` keeps a failed case's folder, `--bless` rewrites golden files. Never run the whole suite while iterating: run the cases for what you are changing, plus any case you add; they must pass before every commit. Prefer proofs to example tests (contracts, refinements and laws proven by `verify`).
- `.strata` files are binary module files (docs/binary-format.md): read them with `strata --file F view main` (a package's module: `strata --file compiler view codegen`), write them with `strata --file F edit < text` (a package's module: `strata --file compiler edit codegen < text`). `edit` replaces or adds the definitions it is given, by name, doc comments included (give a definition with its `///` lines, or they are dropped); it never deletes (`--rm NAME`); an import line given replaces that import. Check the examples still round-trip: `strata --file F view main --proofs > a.st; strata --file b.strata edit --proofs < a.st; strata --file b.strata view main --proofs | diff - a.st` (`--proofs`: views hide loop clauses, `assert … by`, `calc` and lemmas by default). A change to the tree re-encodes the repository's files ("Changing the format" in that doc, `strata upgrade`); the format version stays 1 until the format is settled (user's call), so don't bump it.
- Merging branches: `strata setup-git` makes git merge `.strata` files definition by definition (`strata merge-file`); after a merge, check each module's imports survived and `cd compiler && strata check` passes.
- Releases: `tools/release.sh` assembles `target/release-dist/strata-macos-arm64.tar.gz` (the compiler from `target/strata`, core, std, the runtime and std precompiled, z3, the shipped cache); `tools/release-check.sh` checks it as a Mac without developer tools would run it. `site/install.sh` installs the latest GitHub release.
- Layout: `compiler/` (the `strata` command, in Strata, with `compiler/c/`: the JIT, LLVM and lld glue), `runtime/` (the C runtime, `runtime.c`, and the prelude compiled with every program, `prelude.strata`), `core/`, `std/`, `tools/`, `tests/cases` (with `tests/data`), `docs/`, `docs-site/` and `site/` (strata-lang.dev).
- Verify UI changes (the site) with headless Chrome screenshots: `"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new --disable-gpu --hide-scrollbars --window-size=1700,1000 --virtual-time-budget=4000 --screenshot=out.png "http://127.0.0.1:PORT/"`; serve `site/` with `python3 -m http.server PORT` in the background, and kill **only that port** afterwards (`kill $(lsof -ti tcp:PORT)`).
- Run long commands under a time limit (macOS has no `timeout`: `perl -e 'alarm N; exec @ARGV' cmd`), and BSD `sed`.
