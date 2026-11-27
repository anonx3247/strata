# Issues

Design problems in the lab that are larger than a bug: choices that keep producing bugs, slow the compiler down, or limit what Strata can be used for. Found by reading the code of v0.5.0 (2026-11-27) and by comparing with other languages. Each issue says what is wrong, where it shows, and the direction the rebuild takes (REBUILD.md). Single bugs are in BUGS.md.

## How the compiler passes facts around

### 1. Nodes have no identity, so facts travel by name and printed text

Passes can't attach a result to a node, so they hand results on by renaming nodes, by writing them back as source, or by matching printed text:

- The checker writes inferred types back as annotations (`x: T := e`), wraps generic calls as `{ $result: T := call; $result }`, and tags calls by renaming them (`f#3`, `$lam…`, `$dec…`). It finds the variables again by counting `let`s in order; when the counts differ, the body silently gets no types.
- Codegen infers most types a second time instead of reading the checker's.
- `unique` and `roots` key their verdicts by a function's name and the hash of a statement's text. A text that occurs twice in a function is proven only when every occurrence is.
- Codegen removes proven checks by check name, not by site: one proven "index in range" in a function removes them all (BUGS.md).
- Assumed contract clauses are matched to checks by text prefix; derive state is keyed by printed text; refinements are identified by their printed text; errors are told apart by substring.
- Reserved names carry structure: `$rec:a:b`, `$where_k`, `$mock_T`, `T$own`, `$unbound.f`.

Direction: every node has an id (its definition's stable id and its index in the definition). Each pass returns a table keyed by node id. A check is removed by its node id.

### 2. Answers that default to yes

When a pass doesn't know, it accepts:

- `TAny` fits and unifies with every type. It is the type of every field of sum or union type, every variable matched anywhere in a function (narrowing has no scopes), every `match` with no expected type, and unknown type names. A mismatch is reported only when both types print.
- `unify` has no occurs check and answers true when its fuel runs out. `implements` ends in `_ => true`; any trait value fits any other trait.
- Exhaustiveness answers "exhaustive" past nesting depth 12; one guard turns off the check for a whole `match`.
- Termination counts an iterator it can't type, or one past its depth limit, as finite.
- A check whose goal is `"true"` because a type wasn't found counts as proven, and codegen then removes its run-time check.

Direction: "not modelled" is an error or an open obligation where it arises. Every limit answers "unknown". No catch-all `true`.

### 3. Hand-written tree walks drift apart

About 50 functions in 17 modules walk the whole tree by hand, and they disagree about what is in it:

- `store.refs`, from which verify's cache keys are built, misses impl method contracts, inherent method calls, refinement predicates and bounds. A changed contract can leave a cached "proven" in place.
- `walk` misses derivable conditions; `without_notes` misses proof blocks; the printer drops `select`/`race` arm comments and a derive's `model` lines.
- `compiler/tree` has a uniform interface (slots, `rebuilt`), but only diff and merge use it.

Direction: one traversal interface per tree type, generated; folds and rewrites written once; dependencies computed from the same interface.

### 4. Names in flat, string-keyed tables

- Inherent and trait methods are entered as `module.name` in the module's global table, always public. They collide with functions, with each other, and across types.
- Traits are keyed by short name, types by qualified name, impls as `"Trait for Type"` strings, patched up with `last_part` and `ends_with`.
- `lower` in the checker and again in codegen recognises `Natural`, `Positive`, `Probability`, `NonEmpty` and others by spelling, before the program's own types.
- Escape analysis and `unique` find callees by short name, so a local closure takes the summary of a global function with the same name.
- Renaming a trait method renames every method of that name.

Direction: stable ids for every definition, variant, method and impl. Built-in types declared once in core, never named in the compiler.

### 5. One module, many jobs

- `codegen` (22,700 lines) interleaves type re-derivation, reference counting, monomorphization, helper generation, contract checks and instruction selection, writing IR as strings. Temporaries are tracked with linear scans; expressions are printed again as lookup keys.
- `main` (23,900 lines) holds about 20 unrelated commands, with STRATA_HOME setup copied 18 times, 20 swallowed errors, three hand-written JSON writers and three hard-coded lists of core files.
- The checker's type tables are rebuilt by about nine entry points that disagree; the passes that write types back for codegen use tables missing fields that checking had.
- The prover's state is one record of about 120 fields, saved and restored by hand.

Direction: a small mid-level IR between the typed tree and LLVM; counts inserted on it; one module per concern; one type table, built once.

## Verification

### 6. Terms are strings

The prover's values hold SMT-LIB text. A merged value is written out in full wherever it is used, so formula text doubles at each `if` that updates a variable (the pattern behind a 41 GB build). There are four s-expression readers, and `compiler/sorts` exists to re-check the sorts of text the prover itself wrote. Batches are built and parsed up to three times.

Direction: terms as a hash-consed datatype, printed once at the end.

### 7. Termination is the weak link

Contracts see through functions that termination accepted. A wrong "terminates" makes the unfolded definition contradictory (`f(x) = f(x) + 1`), and every check that can reach it becomes "proven". Own-method calls are missing from the call graph, recursion through function values and inside `spawn`/`scope`/`comptime` is not seen, and `self.field.m()` is assumed to descend. `smt` and `termination` decide separately, with different rules, which functions contracts may see through.

Direction: termination proven before a body is ever used in a proof; one decision about transparency, made in one place.

### 8. Three paths from solver answers to verdicts

`verify`, the obligations output and the state graphs each turn solver output into verdicts their own way, and can disagree on one function. The recorded "build and verify disagree" fits this.

Direction: one verdict function.

### 9. Cache keys and trust

Verify's keys come from `store.refs` (issue 3) and from module headers, whose hashes, dependencies and names the reader never checks. A callee's body change re-proves every caller, even callers that rely only on its contract.

Direction: keys from the definition's interface and its callees' interfaces, a body only when it is seen through.

### 10. Specs are not checked for meaning

Nothing flags a `requires` that can never hold (everything proven) or an `ensures` that follows from the types alone (nothing proven). Agents weaken specs to make proofs pass.

Direction: vacuity checks in `verify`; spec changes shown apart from code in tours.

## Runtime and memory

### 11. One OS thread per task

Each task is an OS thread with a 64 MB stack reservation; macOS allows 4,096 threads per process. A server can't hold thousands of connections. All channels and tasks share one mutex and wake every waiter on every change. A long-lived `scope` frees finished tasks only when it ends, so an accept loop leaks one task per request.

Direction: a green-task scheduler, single-threaded first; every wait through one park interface; a task freed when it ends and its handle is dropped.

### 12. Thread safety rests on one global flag

Allocator safety depends on a flag only the task API sets; code running on other threads (`--lib` exports, C callbacks) uses the shared free lists unlocked. Arenas open when a task starts are never freed, and which arena receives allocations depends on that flag at call time. Senders are tracked per thread, not per value.

Direction: the runtime's invariants written as contracts on core.runtime's declarations, and one owner per allocator.

### 13. Values cost a word per field and a block per element

Every field and every list slot takes 8 bytes: a `Bool` field is a word, a `List[U8]` uses 8 times its bytes, a `List` of records is a list of pointers to separate blocks. Trees are one block per node, and a name is a separate string block each time it appears.

Direction: fields and elements at their own size, `Array[T]` with records inline, an `Option` of a pointer as a nullable pointer, flat trees with interned names.

### 14. Copies across tasks are invisible

Sending a large value to 8 workers makes 8 copies, and nothing reports it. Caches, connection pools, route tables and game worlds can't be shared between tasks.

Direction: frozen values (immutable, never counted) readable by any task; copies reported per `spawn` and `send`.

### 15. Strings trust their bytes

Strings from files, stdin, the environment, arguments and child processes are never checked to be UTF-8, and several string functions read past their end on invalid input. The fuzzer generates only ASCII, so it can't find these. Paths and arguments drop everything after a NUL.

Direction: one boundary where bytes become text, returning a `Result`; a byte buffer type for everything else.

## Tools

### 16. Writing before checking

A plain `edit` merges in memory, checks, then writes. Rename, move, fix, done, revert, resolve, upgrade, abort and sync write before checking, write unchecked, or ignore write errors. Writes are atomic per file, not per change.

Direction: every command that changes code goes through one path: merge in memory, check, write all files of a change atomically.

### 17. Edits reach outside the change

Each edit runs `git add -A` on the package folder, may fetch git dependencies and rewrite `strata.lock`, and commits all of it. A pending change is blind to writes made outside `edit`, and `abort` reverts them silently. merge-file merges printed text and parses it back.

Direction: an edit touches only its definitions; dependencies are fetched only on request; merges work on trees.

## std and domains

### 18. The web server isn't safe to expose

No timeouts, no connection limit, one failed `accept` stops the server, one handler panic ends it, requests with both `transfer-encoding` and `content-length` are accepted, the path is decoded before it is split.

Direction: supervised tasks per connection, deadlines from the clock, a connection cap, and a request parser written against the RFCs.

### 19. Formats and engines are partial

std.regex can only test a whole match (no search, captures, replace), recompiles its pattern on every call and always runs the Pike VM. std.json refuses integers beyond 2^53 and has no depth limit. The ORM builds SQL by re-lexing refinement source and stores `Real` as text. std.web's session runtime was deleted with Rust, so its apps can't run, and it has no commands. Reflection hands derives source text instead of trees.

Direction: each std module owns one format fully, with its spec's edge cases as proven properties.

### 20. Missing pieces for real programs

No terminal input beyond `read_line` (no text UI or game loop), no byte buffer, no path type or file handles, no dates or time zones, no deque, no Linux release.

Direction: the std work stream, in the order the rebuild's layers make each possible.
