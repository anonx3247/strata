---
title: Check verdicts kept per definition
packages: compiler
---

# Why
`strata check compiler` took 5 s after a one-function edit, and `strata build compiler` 9 s: each checked all 4,480 definitions again (the refinement probes alone 2.4 s), and the build's uniqueness analysis took 2 s. Each definition already has its hashes in the module headers, so what didn't change can be known to check.

# Background
A module file's header lists each definition with three hashes (`hash`, `interface_hash`, `code_hash`) and the package definitions it depends on. `verify` already keys its verdicts by them (`keys_over`). `strata check` and `build` go through `loaded_errors`, which checked every definition; an edit checks only what it can affect, with the rest of the package in scope (`check_errors_in`, `edit_errors`).

```strata main.check_errors_in
```

# Intuition
`twice` calls `price`. A change to `price`'s body keeps `price`'s interface hash, so `twice`'s key, which hashes its own code and the interfaces of what it reaches, stays the same: only `price` is checked again. A change to `price`'s signature changes `twice`'s key, so it is checked again, and its error is found. A refinement `type Small = Int where ok(it)` runs `ok` at check time, so `ok` counts by its code for whatever reaches `Small`.

```strata-refs checked.keys_of
```

# Keys and verdicts
`keys_of` reads the headers, hashes each strongly connected component of the dependency graph once (`closures`, Tarjan without recursion), and makes each key from the global part, the module's names and imports, the definition's code and its closure. The verdicts are one file per package, rewritten whole, so the cache doesn't grow per edit.

```strata-diff checked.keys_of checked.closures checked.Verdicts
```

```strata-also
checked.opened checked.kept: the verdict file, read and written atomically
checked.runs_code checked.compile_time_fns: what is checked every time, and what counts by its code
checked.about checked.warning_label: which definition a warning is about
```

# Checking only what changed
`loaded_errors` splits the package into the definitions whose key has a verdict and the rest, checks the rest with the others in scope, and keeps the new verdicts when the run has no errors. `check_summary` infers again only the definitions holding a hole.

```strata-diff main.loaded_errors main.fresh_verdicts main.check_keys main.check_summary
```

```strata-routine
main.file_errors main.package_errors: pass `mut disk` and std's folder
main.check_command: timings for std's library, the summary and lint
cache.cache_folders: the `check` folder
```

# The uniqueness analysis
Once check was kept, `unique.proven_sites` was the largest cost of a build after an edit. Its fixed point now analyzes, after the first round, only the functions whose analysis reads a summary that changed, on worker threads (each in an arena). The IR of `compiler/` is byte-identical.

```strata-diff unique.proven_sites unique.names_in unique.entry_lowered
```

```strata-routine
unique.no_fn: a stand-in for a missing key
codegen.generate: timings for its phases
```

# Toward under a second
A profile of `check` after an edit showed whole-package work the few checked definitions don't need. The checked definitions now have in scope what they reach (with every non-function definition), and the whole package only when that finds an error. The package is read once for errors, summary, lint and `RunMode`. The headers' rows are kept per file by its hash. For `build`, `too_long` no longer counts code points of short texts (the uniqueness analysis renders site keys), `kept_by` revisits only callers of what changed, and `own_decls` decodes on the workers.

```strata-diff main.loaded_check main.package_checked checked.rows_of codegen.kept_by text.too_long
```

```strata-also
checked.Row checked.Graph checked.reach_of checked.kept_rows checked.header_rows: the rows and the dependency graph
main.check_keys main.check_command main.run_mode_in main.own_decls: reading once
codegen.calls_in: what kept_by revisits
```

# Test
`tests/cases/check-keeps_verdicts_by_definition.toml`: a package checked, then a signature change (the caller's error found), then a refinement predicate's body change (the probe's error found), with a cache of its own; the warning of a definition not checked again is still given.
