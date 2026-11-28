# Tools reference

This page holds the detailed rules of [the tools](tools.md). Each section adds to the section with the same name there.

## What the tools tell you

**Editing a script.** Outside a package, `strata --file F edit < text` changes a script, or a folder's module: the result is checked, then written, each file in one step (a temporary file renamed over it). Text that doesn't check writes nothing: the command reports `rejected: F doesn't check (N errors; nothing was written; ...)` with the errors and exits with code 1. `--as-given` writes it all the same (a script written in steps), reporting `written as given: F doesn't check (N errors):` and exiting with code 1. A single module file of a package isn't checked alone, since its neighbours may not be written yet.

**Editing a package.** A package's `strata edit` rejects an edit that doesn't check by itself. Uses that it breaks elsewhere (a changed return type, parameter list or parameter type) become holes of a pending change instead. The text of a rejected edit is saved in the package's `.strata/rejected/edit-N.st`, and the rejection message gives the path. This applies to an edit from standard input and to an editor session that gives up.

**What an edit writes.** An edit that changes nothing, because every definition given matches what is stored, does nothing: nothing is written, checked, logged or committed. Otherwise only the module files whose definitions change are written. In those files, each untouched definition keeps its stored bytes and header entry, and only what changed is re-encoded. `strata upgrade` re-encodes every file.

**What an edit checks.** Only what the edit can affect is checked, as `strata check` would check it, with the rest of the package and std in scope:

- the definitions it changed or added
- when a definition's interface changed (signature, contract, fields, variants, methods) or a definition was removed, the definitions that use it, in any module. That means those whose header lists it as a dependency, and those that use one of its names without naming the type (a field read on a value whose type they never name, or a method call).

A change to a body alone can't break anything else, so only that body is checked. An existing error elsewhere in the package, which the edit can't have caused, doesn't block it. `rename`, `move` and `fix` check the same way. std, core, and packages with dependencies are still checked whole, and `strata check` checks everything.

**What `strata check` checks again.** Each definition that checks is kept in the cache, in one file per package (`check/`, about 65 bytes a definition: 290 kB for `compiler/`, rewritten in place, so it doesn't grow from one edit to the next). It is kept under a key of its code and of the interfaces of everything it reaches through the dependencies the module headers list (`compiler/checked`). The next `check` checks only the definitions whose key changed, with what they reach and every definition that isn't a function in scope (the whole package when that finds an error), and gives the warnings of those it doesn't check again. The headers it reads for the keys are kept too, per module file by the file's SHA-256 (`check/*.rows`). So a body change checks that definition alone, and an interface change checks everything that reaches it. What check runs, rather than only reads, counts by its code: what a type or a trait reaches (a refinement's predicate), and `comptime fn`s with all they reach. A definition with `comptime` code, a `derive … for f` or a hole is checked every time. The key also holds the compiler, std, core, the package's strata.toml and strata.lock, the names and imports of the definition's module, and every `impl`'s and trait's interface. Only a run without errors keeps verdicts. The resolver's errors and unused definitions are found for the whole package every time. A package with dependencies is checked whole. On `compiler/` (4,500 definitions), `check` after a one-function edit takes 0.8 s instead of 5.5 s; `build` checks the same way.

**Warnings.** An edit prints warnings about what it wrote, on stderr, as `strata check` does for everything: a contract that should be a refinement, or a refinement that should be a contract (`compiler/lint`). `strata fix [NAME]...` applies the change each warning shows and logs it like an edit. With `--file F`, it writes a script or folder outside any log, as `edit --file` does.

**A new module.** `strata new MODULE "DOC" [-m MSG] [--no-commit]` makes a module of the package (`compiler/new`): its file at its path (`shop.orders` is `shop/orders.strata`), from the definitions piped in, with DOC as the comment that opens it (the module's doc, as `doc` and `docgen` read it). It goes through the same path as `edit`: checked, written, logged and committed. A module's doc is stored on its first definition, so a module is made with at least one. Refused, with nothing written: a module the package has definitions in or a file for, no DOC, nothing piped in, a name that isn't lowercase words joined by `.`, and `--file` (`--file F edit MODULE` makes a folder's module).

**Claims in comments.** `strata lint` prints `check`'s warnings from `compiler/lint` without checking the code. `strata lint --claims` (off by default) also flags each comment or doc sentence that states a fact about the code (`compiler/claims`): `always`, `never`, `every caller`, `guarantees`, `holds for all`, `is safe because`, `we ensure`, `cannot fail`, `invariant:` and the like. Each warning says to write the fact as a contract, invariant or law, where it is checked. A clause opened by a condition (`if`, `when`, `that`, `each` …), code in backticks, quoted text and a participle (`a variable never assigned`) are left alone. One pass over the stored trees, no checking: on `compiler/` it flags 146 sentences.

**Unused code.** `strata check` reports what a whole package, folder or script never uses (`compiler/unused`), after the unused variables. Each is a warning, or an error when strata.toml says `[check] unused = "error"`. Another value of `unused` is refused, naming the two allowed.

- A private function, type, trait or capability that nothing reaches: `` warning: `deps.before` is never used (remove it with `strata fix`, or make it `pub`) ``. The roots are `main`, tests, `pub` definitions (only those `exports` lists, when it lists some), `extern` declarations, derived types and functions, and what contracts name. A definition reaches what it names. A method call `x.m()` reaches every trait and capability declaring `m` and every function named `m`. A trait reaches its methods' bodies, `derived fn`s included. An `impl` is reached when its trait and its type are, so an impl for a dead type is dead too.
- A field of a private record that is never read: no `x.f` whose receiver the checker types as that record (or can't type), no record pattern naming it, and no spread `...x` into a parameter of that name. A field name that no other record has needs no typing. The others are typed one reader at a time, starting with the record's own module, until a read of that record is found.
- A variant of a private sum that is never built, only matched (or never named at all).
- A name starting with `_` is never reported. Neither are the fields and variants of a `mock`, of a derived type, or of a type a derived type or function holds, nor in a library (a package without `main`) of a type its `pub` definitions show. A script or folder without `main` and without strata.toml is not looked at. core's roots are also its `builtin` definitions, its modules outside `noalloc` and its `strata_…` functions, which compiled code calls by name; its fields and variants are left alone. std's roots also include the private functions the compiler's generated code calls (`std.python.call`, `std.regex.match_code`, …).
- `strata fix` removes the unreached definitions (those named, or all), with the impls of the types and traits among them, logged like its other fixes: `` fixed: removed `dead_fn`, which is never used ``.
- An edit prints what it leaves unused that wasn't before, untyped (a read of a field another record also has counts as a read). With `unused = "error"` the edit is rejected. `run`, `test`, `verify` and `build` refuse such a program too.
- The cost is one pass over the program's trees, plus typing the readers of a few fields: about 0.2 s of `strata check`'s 5 s on `compiler/` (measured with `--timings`; the whole check took 4.9-5.3 s before and 5.2-5.4 s after).

**Loop invariants found where code is written.** `verify` and `build` never search for loop invariants: they use those derived from the loop's code and the `invariant` clauses written ([verification](verification.md#loops)). The search runs when code is written. After an edit, the changed functions that have loops are proven with the derived invariants, and where a check in or after a loop fails (or a loop isn't shown to end), invariants are searched for in that function and written into its code as `invariant` clauses, each loop with a warning:

```text
warning: in `quoted_end`: added `invariant lx.at <= n` to `while lx.at < n && char_at(lx, lx.at) != "\""` (found by search)
```

An invariant naming a variable's value before the loop gets a binding just before it (`start_i := i`), and the warning says so. In a package the clauses are a change of their own, logged as made by `edit` and committed like the edit. When nothing is found, the edit says which checks still fail (`warning: in `f`: not proven: …`). `--no-search` skips the search. For code written outside `strata` (a merge, another tool), `strata fix NAME` runs the same search on demand, for the functions named (also `module:m` or `prefix*`, as `verify` takes them; every function with loops when none is named), after the warnings' changes, and prints `fixed: in `f`: added …`. A run's searches share one budget of solver resource units (2,000M).

**Derives written on a declaration.** A declaration given in an edit brings its `derive A, B for` line. A stored derive that `view` would show on that line (no block of methods, for a type or function in the same module, not `for module`) is removed if the line no longer names its trait, with `warning: removed derive std.toml.Toml for Point (no longer on its derive line)`. A declaration given with no derive line loses all such derives, matching what `view` would show. The removal is a change like `--rm`'s: it is listed, logged in `strata log`, and shown by the diffs. A declaration the edit doesn't give keeps its derives. Derives with a block, derives for other modules' declarations, and `for module` derives are removed only by `--rm`. `strata view NAME` and the editor opened on a selection (`strata edit Point`) show a declaration with its derive line, so editing a definition as `view` shows it never loses its derives.

## How it is stored

**Code.** Code is stored as its syntax tree, in binary. Each `.strata` file is a module file, in Strata's own format, specified in [binary-format.md](binary-format.md).

- Text goes in only through an edit (`strata edit`, `strata add`) and comes out through `view`. `strata view MODULE` prints a module in reading order ([tools](tools.md), "Views"); the stored order is kept.
- `strata view --split DIR` writes a package's text as one `.st` file per module, for reading, not for reading back.
- A type listed by one union is shown inside it unless it has its own doc or `impl`. `--members separate` shows every member as a separate definition. Both describe the same program.
- Outside a package's log, `strata --file F edit < text` stores a script, or a folder's module with `--file DIR/ edit MODULE`, creating it if new. Only text that doesn't parse is refused.
- Each module file starts with a header listing its definitions with their hashes and dependencies (`strata header core.strata` prints it). Each definition stores its interface before its body.
- `strata pack app.strata` writes the whole program as one bundle, which `--file`, `strata run` and every view read like the package.
- The hashes `strata` shows are hashes of the binary encoding (see the spec).

**Verdicts.** `verify`'s verdicts are kept outside the package, in `.git/strata/verdicts`, shared by every worktree of the repository. Outside a repository, they are kept in `~/.cache/strata/verdicts`. There is one file per verdict, keyed by the hash of a definition and everything it depends on, plus the check's name and the run's settings.

## Proofs in views

Proof material is stored with the code, but views hide it by default. That covers a loop's `invariant` and `decreases` clauses, a function's `decreases`, `assert … by` and `calc` statements, and lemmas. Plain `assert`s stay visible: they are checks that run.

- `strata view` (a module, `--level 1` and `2`, the map) and the editor `strata edit` opens put a quiet marker where proof material is hidden: `// ⋯ 2 invariants`, `// ⋯ decreases`, `// ⋯ 1 proof`. A module's lemmas are left out, and the map doesn't list them. A lemma picked by name (`strata view sum_natural`) is shown.
- `--proofs` shows everything (`strata view main --proofs`, `strata edit --proofs` in the editor). `--split DIR` always writes everything.
- `strata edit` of text without the proof material puts the hidden parts back from the stored definition and drops the markers. A loop's clauses go back on the loop with the same head (the k-th `while i < n`, as stored) when the text gives that loop none. The function's `decreases` comes back when the text has none. An `assert … by` or `calc` goes back after the code statement it followed, or before the one it preceded, when that statement appears once. A part whose place changed or is gone is dropped, and a warning says so: `warning: in `sum_to`: dropped 2 clauses of `while i < n` (hidden in the view): that loop changed or is gone; …`. The search for invariants that runs on edit (`--no-search` skips it) then covers that loop. `strata edit --proofs` takes the text as complete: nothing is put back, so the proofs it leaves out are removed.
- `strata diff` and `strata tour` show a changed definition by its changed nodes (`compiler/hunks`, from `compiler/tree`'s diff): its signature, then each changed node printed whole, under the first lines of the nodes around it (`for x in xs {`, `if c { … } else {`). A change is shown at the outermost node around it that is on one line on both sides (its statement, its arm, its field), compared word by word; a node on its enclosing node's first line shows that line changed. A node wrapped in a new one (a statement put inside an `if`) shows the new lines added and the old node kept. Nothing else of the definition is shown, so spacing, notation and a re-encoding are never a change. `--json` gives these lines in each item's `lines`.
- `--lines` (on both) shows the changed lines instead: each side printed with its changed nodes placed, unchanged lines folded, a line changed in place compared by its changed nodes' texts, word by word. A definition whose sides don't print the same with their nodes placed, or mostly rewritten, gets a line diff of its text.
- `strata diff --nodes` shows each changed definition as it was and as it is, with the nodes that changed marked (`^^^` under them, as `view LOCATION` marks a node), then each change by path: `edited body/0/value: x + 1  →  x - 1`, `added body/1`, `moved body/1/arms/0 → body/2/arms/1`, `replaced`, `removed`. `--json` gives the same changes in each item's `nodes` (`kind`, `old` and `new` paths, `before` and `after` texts). The changes come from comparing the two trees node by node (`compiler/tree`): an `edited` node's head changed (an operator, a name, a literal) and its children are compared on their own; a list is lined up by its equal items, then by items of the same form and head. Cost: one walk down the parts that differ: 0 to 5 ms for all the definitions a compiler commit changes.
- `strata diff` reports proof changes apart from code changes: `proofs changed` versus `body changed` (for a method, `` `m`: proofs changed ``). A lemma is listed as a `lemma`.
- To round-trip a file through text, view it with `--proofs`: `strata --file F view main --proofs > a.st; strata --file b.strata edit < a.st`.

## Finding code: `find`, `refs`, locations

**Builtins and std** are read like the program's own code. A name the program doesn't define is looked up in core and std, also outside a package. `strata view Dict` and `strata view insert` show the builtin's doc and signature, with every overload. `strata view std.json.parse`, or `view JsonValue` for a public std name that core doesn't have, shows std's definition. `strata find Dict` lists them. A builtin's doc ends with its cost, always in the form `Cost: O(1) amortized, in place when ...; O(n) otherwise (a copy).`. These costs are checked against the runtime and measured.

**Locations.** A location's path is made of the tree's field names and list positions:

- a block's statements, then its result, by position (an expression statement is its expression)
- `callee` and `args/N` of a call, and `recv` of a method call
- `arms/N/pat` and `arms/N/body` of a `match`
- `params/N/ty`, `ret`, `pre/N`, `post/N` and `body` of a function
- `methods/N/…` of an impl, trait or capability
- a record argument or pattern field by its name (`args/attempts`)
- `value` inside `return`, `assert`, `mut`, `spawn` and similar

The hash in a location is the definition's identity hash, comments included, so a location shows whether the definition has changed. `view` then shows the same path in the current definition, or as much of the path as still exists. `view` accepts a location anywhere among its arguments.

**`refs`** looks for uses in the definitions whose header lists the target's definition, or a member's type or trait, as a dependency. A qualified name the package doesn't define (`std.list.map`) is searched for by name everywhere.

- A field read (`x.attempts`) counts when the checker types `x` as the field's record, or when no other record has a field of that name.
- A call to an inherent method counts when the checker resolves it to that method.
- Calls to trait and capability methods count by name.
- A root-module name used where a local with the same name is in scope refers to the local.

**Error locations.** `strata check` follows each error with `--> LOCATION` and the code after `| `. The location is the first node, outermost first, that the message quotes as code (an expression or pattern before a type). Without one, it is the definition's head. `compiler/codegen`'s "not compiled yet" errors (`build`, `run`, `test`) end with the code they occur in (`, in `CODE``), followed by its location in the same way. The checker itself doesn't track source positions, nor do run-time faults, `--trace` or `verify`'s refutations (see IDEAS.md).

## Measuring memory: `--memory-sites`

`strata build --memory-sites` builds a program that counts, at each place in its code, the blocks it allocates, the holders it adds and drops, the tests it makes before updating a value in place, and the lists and strings it extends. `strata run --memory-sites` does the same on the JIT. When the program ends it prints the totals by kind and the 20 places counted most on stderr:

```
memory sites: 68 sites
  alloc              18 at 3 of 5 sites (heap 12, arena 6)
  unique             31 at 3 of 3 sites (held once 30, shared 1)
  ...
top sites:
            15 push     new buffer 3  grow  push(p.points, Point { x: i, y: i * i })
```

A `runtime` site is a call of the runtime that may allocate, with the objects made during it.

After the sites come the totals by root. A root is the scope whose region would hold a value if values were allocated by scope (IDEAS.md, "Memory: scoped allocation from roots"): a statement, a loop's body, a block or a function. A value that would stay counted is listed with the reason. Each line gives the blocks allocated at sites (and how many of them an arena made), the objects made inside the runtime's calls, and the releases with the releases they led to inside helpers. The last line gives the share that regions would hold.

```
memory roots:                                    allocs   in arena      runtime     releases       nested
  statement                                           1          0           15           14            1
  loop body                                           6          0            6            3            6
  counted: replaced in a loop                         4          0            0            5            0
  scoped (codes 1-4), share of all                72.2%                  100.0%        80.0%       100.0%
```

With `STRATA_MEMORY_SITES=FILE` it also writes every place to FILE as tab-separated lines: the counts, the kind, the function, the location (which `strata view` accepts), the code and the root. A function's result has the root `result`: it takes its caller's at run time. The program runs slower in this mode, and std is compiled with it. How it is built: [internals](internals.md), "Memory".

With `STRATA_MEMORY_LIVE` set too, the program counts the heap's live objects by the place that made them, and reports the places holding the most at the peak of live bytes:

```
live at the peak: 12635102 objects, 499.3 MB (copied 267 times); now 0 objects, 0.0 MB
     243.8 MB    7989757  runtime  strata.compiled.str_chars  push(out, byte_slice(s, i, j))
      25.8 MB     846462  append   std.json.read_string  out + c
```

An object is a block with its count word: a small one counts its size class. Objects made inside the runtime's functions count for the place that called them. Statement regions and arenas are left out. `STRATA_MEMORY_LIVE=FILE` also writes every place to FILE: the objects and bytes at the peak, then at the end, then the place's line as in `STRATA_MEMORY_SITES`. Counting costs two atomic adds per allocation and per free.

`tools/memcurve.c` samples a command's memory every 10 ms (resident size and footprint, children included) into a CSV and plots it. `tools/memprof.sh` runs the workloads in `tools/memprof/` and `strata check` of compiler/ under it.

## Docs in the terminal: `strata doc`

`strata doc [TOPIC]` (`compiler/markdown` renders, `compiler/docgen` finds definitions) looks TOPIC up in this order:

1. A page: `NAME.md` in the `docs` folder of Strata's home. The home is the release's `lib/strata`, where `tools/release.sh` puts `docs/*.md`, or a source checkout's root (`STRATA_HOME`). `docs/std/` is left out of a release: `doc MODULE` makes those pages from the code. `PAGE#SECTION` shows the section under the heading whose anchor is SECTION (`values-and-copying`, or the heading's words), else the sections whose anchors hold it. Several matches are listed.
2. A module of the program around the current folder (or `--file`'s), of std or of core: the page `docgen` writes for it.
3. A definition. A name is qualified (`std.json.parse`), short (`parse`), its last parts (`json.parse`), or a method (`Order.total`, `Console.print`, `Dict.insert`: a builtin whose first parameter has that type). The program's definitions come first, then core's, then std's public ones. A name defined in several modules of the first that has it is listed with each definition's first sentence. Several signatures in one module (a builtin's) are shown together.

The renderer reads the Markdown the docs use: headings (bold; `#` and `##` underlined without color), paragraphs and list items wrapped to the width, fenced code indented by 4 (`rust` and `strata` fences highlighted by `compiler/highlight`, other languages as written), inline code colored (its backquotes kept without color), tables aligned, quotes, rules, and links shown by their text, with the target after it for a web address. HTML comments are left out. The width is the terminal's, through the pager too (`driver_columns`, c/driver.c), at most 100 columns, and 80 when the output is not a terminal. The output depends only on the text, the color setting and the width.

## Reference pages: `strata docgen`

`strata docgen [MODULE]... [-o DIR] [--man]` (`compiler/docgen`) reads the package around the current folder, or the one given with `--file`, as `view` does. It writes names as the module's code writes them.

- **A module's doc** is the comment above its first import, or else above its first definition, up to a blank line. A leading `name — ` on the first line is dropped, since the title already gives the name. A section rule (`// ---- …`) is not part of the doc.
- **Docs are read as prose.** Adjacent lines form a paragraph. A line starting with `- ` is a list item, and its indented following lines continue it. Lines indented by two or more spaces, after a blank line or a line ending in `:`, are an example, shown as code.
- **What is documented:** public types (including capabilities and `builtin type`s), the methods of a public type's inherent `impl`, public traits, public functions (including `extern` ones), `impl`s of the module's public traits or for its public types (listed by their first line), and `pub use` re-exports. A module with none of these gets no page unless it is named.
- With MODULEs given, only their pages are written, with no index.
- **Man pages** are man(7) roff: NAME (the module and its doc's first sentence), SYNOPSIS (`use MODULE`), DESCRIPTION, then a section per kind of definition. `strata(1)` is `strata help`, with its one-line headings as subsections and its command lines as written.

The site's std pages are `docs/std/`, written by `strata --file std docgen -o docs/std`. `docs-site/sync.mjs` turns them into the pages under `/docs/std/`.

## Packages

**Dependencies.** `[deps]` names other packages by local path (`json = { path = "../json" }`) or by git URL (`json = "https://…"`, or `{ git = "URL", tag = "v1" }`, `rev` or `branch`). A git dependency is pinned to a commit in `strata.lock`, fetched into `$STRATA_CACHE` or else `~/.cache/strata`, and moved to a newer commit by `update`. The key is the dependency's `name`.

- A dependency's modules are prefixed with its package name, as std's are with `std`. Its root module (`main.strata`) is `json`, and its module `text` is `json.text`. A module of the depending package can't share a dependency's name (`` module `json` is named like the dependency `json` ``).
- Inside the dependency nothing changes: its root module's definitions, union and sum variants included, stay visible by short name to its other modules.
- Everywhere else, it is reached only through `use` (`use json`, `from json use parse`, `use json.text`), and only for what its strata.toml `exports`. `exports` lists modules or single definitions (`exports = ["text", "parse"]`), with a root module definition given by its name or as `main.NAME`, and the root module itself as `main`. The default is every `pub` definition. Using anything else is an error worded like a private definition's (`` `parse` isn't exported by package `json` ``).
- Loading a package (for `check`, `run`, `verify`, …) fails on an export that names nothing or a definition that isn't `pub`.
- A dependency's definitions are read lazily, as the program reaches them. Only the module files it reaches are read, and only the definitions it reaches are decoded. A variant or a trait method brings in its type or trait.
- Packages can't depend on each other in a cycle: if `a` depends on `b`, `b` can't depend on `a`, directly or indirectly (`dependency cycle: a -> b -> a`). Cycles between the modules of one package are allowed. std depends on nothing.
- A dependency is checked, verified and tested in its own package. Commands report only the package's own definitions, and a dependency's `test` blocks are skipped (`strata test --with-deps` runs them, and `check --with-deps` checks them).

Dependencies are handled by `compiler/deps` and `compiler/main`, whose `package [--std DIR] DIR`, `check [--std DIR] [--with-deps] DIR` and `update DIR [NAME...]` commands expose them directly.

**The cache.** A run reads two caches and writes one (`compiler/cache`, `caches_of`): its own (`$STRATA_CACHE`, or else `~/.cache/strata`), and a release's shipped cache, read-only, next to the executable (`lib/strata/cache`, or `STRATA_SHIPPED_CACHE`).

- Every key starts with the compiler's identity, the SHA-256 of its executable, so a new compiler never reads an old one's entries.
- Nothing is evicted. The own cache only grows until it is cleaned by hand.
- `strata cache` prints where the own cache is, its size, and each top-level folder's size and contents:
  - `verify-strata`: verify's verdicts, proofs and loop findings
  - `check`: check's verdicts, one file per package
  - `objects` and `ir`: built programs
  - `std-library` and `core-rt`: std and core.rt, compiled
  - `comptime`, `proofs-lines`, `identity`
  - `deps`: git dependencies
  - `build`, `run`, `smt`: scratch space for runs, one folder per process id

  Folders that nothing reads any more are marked as belonging to an older compiler. The shipped cache's size comes last.
- `strata cache clean` removes the whole own cache and reports how much space it freed. It never touches the shipped cache, a std library, or the verdicts kept in a git repository (`.git/strata/verdicts`). It refuses a folder that isn't a cache: `/`, `$HOME` or anything above it, a package, a git repository, or a folder holding std or the shipped cache. It also refuses while another running strata has scratch in it (a `build/PID`, `run/PID` or `smt/PID` whose process is alive); `--force` cleans anyway. Run it when nothing else is running.
- `strata cache clean --obsolete` removes only the top-level folders nothing reads any more (an older compiler's layouts, such as `std/`, `compiler/`, `scripts/` or `termination/`) and the scratch left by runs that ended without removing it.

## Git

**`strata merge-file %O %A %B %P`** is a git merge driver. It pairs each side's definitions with the base's.

- A definition that only one side changed comes from that side, so edits to two functions never conflict, even on neighbouring lines.
- When both sides changed the same definition, it is merged node by node from the base (`compiler/tree`), as stored: every reference is qualified there, so an import that shortens names on one side changes nothing. The result is written as ours shows it, with the imports theirs added. A node that one side changed comes from that side, so edits to two statements, two arguments of one call, or one arm while the other side reorders the arms, never conflict. A node that kept its form on both sides is merged by its children. A list (statements, arms, arguments, fields, methods) is lined up three ways by the items both sides kept: a run of items changed on one side comes from it, and runs changed on both sides are merged item by item when they are as long.
- A node changed on both sides differently is a conflict there only. Inside an expression or a block it becomes a hole in the merged code, `?conflict_N(ours, base, theirs)`, and the file stays a module file: `strata holes` lists the holes, `strata view` shows them, and an `strata edit` of the definition replaces them. merge-file exits 1 and prints `conflict in f: ?conflict_1 at body/0/value/right`, so git leaves the file unmerged until `git add`.
- A conflict outside every expression (a signature, a field's type, two different items added at one place of an arm list), or a definition both sides added differently, falls back to its canonical text merged line by line, kept only if it parses back into that one definition. Otherwise, conflict markers go around that definition alone and the file is left as text. Fix it, then run `strata resolve FILE` to store it as a module file again. `resolve` names the module as its package does: core's `list.strata` is `core.list`, std's `json.strata` is `std.json`.
- Cost: one walk of the three trees down the nodes that differ, plus a longest common subsequence per changed list (O(n·m) node comparisons on what is left after the common start and end).
- A rename on one side (a definition removed, and a new one equal to the base's after `strata rename`) is applied to the other side, so new calls to the old name follow it.
- The merged file is then type-checked inside its package. An error that neither side had alone fails the merge, for example when one side changed a signature and the other added a call using the old one.
- A side that isn't a module file isn't merged, and ours is kept.

## The REPL: `strata repl`

`strata repl`, or `strata --file F repl`, opens a session on the package around the current folder, with all its functions, types, traits and impls, plus std and core in scope.

**Inputs.** Each input is one of:

- an expression: `double(21)` prints `42: Int`, the value as code (as `strata run f` prints a result) and its type. A value that can't be written as code, such as a function, shows only its type.
- a binding: `x := e`, `mut x := e`, `x = e`, `mut xs.push(7)`. `x := e` prints `x: Int = 10`.
- a definition (`fn`, `type`, `trait`, `impl`), with its `///` doc lines. It belongs to the session. A definition with the same name replaces the earlier one, or the package's own, for the rest of the session.

An input continues over several lines while a bracket or block it opened is still open, or after a doc line.

**Commands:**

- `:type e` prints the type only, without running anything.
- `:view NAME` shows a definition, the session's first.
- `:save NAME` writes a definition typed at the prompt into the program, as `strata add` writes it. In a package it is checked and logged as `repl: save NAME`. In a `--file` script it is written and checked. It is then part of the program.
- `:reload` reads the package's files again. Each value whose type still means the same thing is kept; the others are dropped, with a message.
- `:quit`

**Line editing.** At a terminal, lines can be edited with the arrows, Home/End, Backspace/Delete, and Ctrl-A/E/B/F/K/U/W. Ctrl-C discards the line, and Ctrl-D on an empty line ends the session. The up and down arrows recall earlier lines. When standard input isn't a terminal, each line is read in turn as if typed, and the exit code is 1 if an input failed, so test cases and agents can drive the REPL.

**How inputs run.** Each input is checked against the program and the session, then compiled by `compiler/codegen` as its own function, `strata_repl`, whose parameters are the session's values. The JIT runs it in the same process, as it runs `comptime`.

- Values stay in compiled memory between inputs, functions and closures included. Each lives in a session slot and is passed to the next input's function as a parameter. A `mut` value is assigned through its slot, so an input's changes are kept even when a fault ends it.
- A value that is no longer needed (a result already shown, a name rebound) is dropped by the next input.
- A value whose type's definition changes (`type Point = …` again, or through `:reload`) is dropped, with a message.
- What every input's compilation shares (core, the prelude) is read once, and std's precompiled library is loaded once. An input therefore compiles only its own function and what it reaches outside std. `--timings` shows the REPL's phases.
- A fault (`[1, 2][5]`) is shown as `strata run f` shows one, with its kind and the call stack below the input (`fault (Runtime): index 3 out of range (length 1)`, `stack: outer -> pick`), and the session continues.

**Capabilities.** By default the REPL uses its own capabilities, never the program's real ones.

- `console` is a `std.io.ReplConsole`, a capability for the REPL. What an input prints is shown with its result, and `console.read_line()` reads the REPL's next input.
- `disk` is an in-memory folder belonging to the session (`std.fs.mem_files`), so what a line writes stays in the session.
- There is no `process` or `launcher`.

`strata repl --allow files,process` provides the real ones: `disk` is std.fs's `Disk` as a `Files`, and `process` and `launcher` are std.process's `Process` and `Launcher` as an `Environment` and `Commands`. Inputs then run in the folder the REPL started in.
