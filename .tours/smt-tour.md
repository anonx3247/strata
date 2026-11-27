---
title: How the SMT verifier works
packages: compiler
---

# The big picture
`strata verify` proves each function's checks with Z3. A check is an
`ensures`, a refinement at a boundary, an index, a division, a callee's
`requires`. What the solver proves is still run a little (the cross-check).
What it refutes is replayed. What it can't decide is fuzzed.

The encoder is a symbolic run. It walks a function's body once, keeping
every value as SMT-LIB text, and gathers the obligations along the way.
Each obligation is a goal, a path and an index into the facts known so
far. A function's obligations go to the solver as one script, the batch.
Between `push` and `pop` each one gets only the facts its goal depends on
(its cone). The answers come back as text and are read into verdicts.

```text
verify_file
  check, termination.analyze_with --> Methods.seen / unfold / specs
  verdict cache (closure key) and proof cache (interface key)
     hit: cross_checked                      miss: 256 at a time
  solved_together -> solved_with
    encode_solving        parallel.map, one Vc per function
      function(vc, f)     eval: facts, defs, pc, obligations (all String)
        ask(question) ----------------> solver_pool_run  (loop searches)
      batch(vc) ----------------------> solver_pool_prefetch
    answers_all
      batch(vc) again ----------------> solver_pool_run  (memo hit)
      single(vc, i) for the unfinished, the retried, the examples
      answer_of: proven | refuted (args as Expr) | unknown (why)
  check_fn: replay counterexamples, fuzz the rest, cross-check the proven
  write_verdict, write_proof, report
```

Every arrow into the solver carries a `String`, and every answer comes back
as one: a script out, its replies in. Nothing between `eval` and Z3 is a
tree. Terms are built by concatenating text, and later read back by
scanning it: for unknowns, string terms, sorts and replies.

```strata verify_file
```

```strata-refs solved_together
```

# The modules
Sizes are the text view's lines (`strata view M --proofs`) and the binary
file's bytes.

| module | lines | bytes | definitions | owns |
|---|---|---|---|---|
| `compiler/smt` | 17,505 | 451 kB | 505 functions | the symbolic run, the encoding of values, calls, loops, the batch builder, reading replies and models, running scripts |
| `compiler/sorts` | 647 | 14 kB | 18 | a sort check of a finished script before it is sent: it parses the script back into `Sx` trees |
| `compiler/termination` | 2,325 | 61 kB | 66 | the call graph, loop bounds and recursion measures, and from them which functions a contract may see through |
| `compiler/contracts` | 1,368 | 36 kB | 48 | static errors on contracts and refinements (a literal that breaks a type), no solver |
| `compiler/main` (the verify driver) | `verify_file` is about 900 of 22,316 | | | keys, caches, chunks, `check_fn`, replay, fuzz, cross-check, the report |
| `compiler/cache` | 562 | 22 kB | 45 | SHA-256 entry keys, reading and writing cache entries, the scratch folder |
| `compiler/c/solver.c` | 850 lines of C | | | one long-lived Z3 per core, the job queue, the in-process answer memo, the search budget |

The fuzzer and the cross-check are compiled drivers run in this process by
`c/jit.c` (`fuzz_run`). `smt.inputs` asks the solver for parameter values
that meet a refinement random values rarely meet.

```strata sorts.sort_error
```

```strata termination.analyze_with
```

```strata cache.entry_key
```

# The symbolic run
`smt.function` runs one function. It makes each parameter a value of its
type (`havoc`: fresh unknowns, its validity assumed), puts each `requires` on
the path, evaluates the body with `eval`, and at every exit makes an
obligation for the result's refinement and for each `ensures` (`pred_old`).

All its state is one record, `Vc`. The parts that matter for the encoding:

- `facts: List[String]`: SMT-LIB Bools, each guarded by its path
  (`assume` writes `(=> PATH t)`). Order matters: an obligation sees a
  prefix.
- `defs: List[String]` with `def_set: Set[String]`: facts true on every
  path (a term's definition, a path name's definition), stated once,
  deduplicated by their text (`define`).
- `pc: List[String]`: the path condition, one term per branch or check.
  `path_term` names each prefix with a fresh Bool (`(= p7 (and p6 c))`), so a
  guarded fact costs a few words, not the whole path. The names are found
  again through `memo`, keyed by the text `"path " + p + " " + c`.
- `consts: List[Const]`: the unknowns, named `v0`, `v1`, … by `fresh`. A
  fact's unknowns are not kept with it. They are found again by scanning
  its words (`consts_in`, `is_const`).
- `memo: Dict[String, Val]`: everything shared by text key: path names,
  `share`d terms, congruence entries, calls already made.
- `obligations: List[Obligation]`, `given: List[String]` (goals that are
  literally `true`: proven without a query), `term_obs` (termination's).
- `gaps: List[String]`: what the encoder doesn't model. A function with a
  gap asks no query and is fuzzed.

An `Obligation` holds no facts of its own. `facts: Int` says how many of
`Vc.facts` were known when it was made, `first_fact` cuts the front (a
proof's step), and `own` holds facts that left `Vc.facts` when a proof
ended. Its `goal` and `pc` are text.

`oblige` records the obligation and then pushes the goal onto `pc`: past a
check, the run goes on only where it held. A check the code doesn't state
(an index, a refinement) is pushed as `checked(g)`, so a proof's small query
can leave it out.

```strata smt.Vc
```

```strata smt.Obligation
```

```strata smt.function
```

```strata smt.oblige
```

```strata smt.path_term
```

# Encoding values
A `Val` is the structure the run carries. Its leaves are text: `VInt(t)`,
`VBool(t)`, `VStr(t)` each hold an SMT-LIB term. Records (`VRec`), sums
(`VSum`: a tag term and every constructor's payload), tuples and unions
(`VUnion`: a shape tag and one slot per shape) are structures of such terms.
`VData` is a value kept as one term of its datatype, unfolded a level at a
time when read (`force`, `unfold`). `Opaque` is something not modelled.

**Sorts.** `sort_of` maps a type to a sort and records it in `Vc.sorts`
(`SortDef`): a datatype for records and sums, an uninterpreted sort, a list
sort, a map sort. `declarations` writes them all at the head of each
script: the datatypes as one `declare-datatypes` group, a `.tag` function
per datatype, `.len`/`.at` per list sort, `.size` per map sort, and the
validity predicates.

**Lists.** A list is `VList { len, id }`: a length term and an index into
`Vc.lists`. Its `ListInfo` says where its elements come from (`Gen`: a
literal, a range, a `map` or `filter` of another list, a shift, a concat, a
list inside a datatype value) and what holds of every element. An element at
any position is a template with `@@i` placeholders in its text, replaced by
string substitution (`place`). Facts about every element are quantified over
positions (`forall_positions`) or, by polarity, stated only at the positions
the code reads.

**Dicts and sets** are SMT arrays from key to `Option` of the value (a dict)
or to `Bool` (a set), with a size function of the array (`MapSorts`).
`all(d.items(), f)` assumed is not a quantifier: it is stated at each key
the code reads (`map_read`, `MapAll`).

**Strings** are SMT strings. A string read out of a datatype value is named
`str{i}` in each query (`str_names`, then `piece` in the batch), since Z3's
string solver gives up on selector terms. `byte_len` is an uninterpreted
`utf8_len`. A regex with a literal pattern becomes an SMT regular
expression (`regex_match`). Every `str.len` term in a query gets a bound of
2^64.

**Refinements.** "`t` is a valid `T`" is a call of a predicate `|validK|`,
declared once per erased type and sort (`valid_term`). Its meaning is stated
per term, not per type: when a term is unfolded, `unfold` builds the
refinement's condition (`check_type`) and states `(= (|validK| t) cond)` as
a def, its long subterms read through a `let` (`let_term`). `check_type`
builds the condition a value must meet at a boundary.

**`old`.** An `ensures` is evaluated with `Vc.olds`, the values at the start
(`pred_old`). The `mut` parameters are read at each exit (`Exit.state`). A
call seen through its contract binds `old` to the arguments before the call,
and its `mut` arguments get the values it leaves (`write_after`).

**Sized ints** are `Int` terms kept in range (`word_range`) and wrapped by
`mod` (`wrap_term`).

```strata smt.Val
```

```strata smt.SortDef
```

```strata smt.ListInfo
```

```strata smt.unfold
```

```strata smt.valid_term
```

# Calls, proofs, loops and termination
`smt.call` decides how a call is seen.

- **In code**, a call is its contract (`call_contract`): its `requires`
  become obligations, its `ensures` and result type facts.
- **In a contract** (`Vc.spec > 0`, or a loop's written invariant), a call
  of a function that finishes and can't fail (`Methods.seen` is
  `SeeThrough`) is seen through. When its body is one term of its
  parameters of 2000 bytes or more (`small_definition`), or it makes
  calls, it is a `define-fun` stated once per query (`def_call`,
  `define_fn`, kept in `Vc.fun_defs` as text and copied in by `headed`).
  Otherwise its body is evaluated inline at each call. A
  recursive function that calls only itself (`Methods.unfold`) is an
  uninterpreted function plus its body's macro, unfolded two levels at each
  use (`rec_call`, `max_unfolding`). `NotKnown` (its finish needs a proof)
  is a gap.
- **Spec functions** (`spec_functions`: own functions only contracts and
  lemmas use, whose bodies quantify) are a symbol of their arguments outside
  a proof, and seen through inside one (`in_proof`).
- **Congruence**: a call seen through is compared with up to 16 earlier
  calls of the same function (`congruent`, `congruent_calls`): equal
  arguments imply equal results. Arguments and result are named first
  (`share_deep`). `share` turns any term over 64 bytes into a fresh unknown
  defined once, keyed by `"share " + sort + " " + text`.
- **In a proof's code**, a call of a function contracts see through is also
  said equal to its body (`proof_body`), as the proof's own facts.

**`assert c by { … }`** (`assert_by`) runs the steps, then proves `c` in a
small query: `small_goal` sets `first_fact` and `facts` to where the proof
began, so the query sees the path and the proof's facts only. When the
proof ends (`proof_ended`) its facts leave `Vc.facts`; the checks inside it
keep them as `own`. Afterwards `c` is one condition on the path.
**`calc`** proves each step the same way, then the whole chain. **A lemma**
is a function run with `proof_from = 0`.

**Loops.** `while_loop` checks the written invariants on entry
(`written_on_entry`), makes what the body changes any value at the head
(`havoc_loop`), assumes the written invariants and those derived from the
code (`assume_written`, `loop_facts`: bounds walked from each pass's steps,
`pass_walk`), runs one pass, and checks the written ones where each pass
ends (`written_kept`). A `for` over a list runs as an index loop
(`for_loop`). `verify` searches for no other invariant (`Methods.search` 0):
it notes the loops skipped (`Vc.skipped_loops`) for `loop_hint`. Only
`--find-invariants` searches (`infer_invariants`): each candidate is a
question `ask` sends while the run waits (search: 20M units, probe: 0.5M).

**Termination** is its own pass over the call graph, before encoding
(`termination.analyze_with`). A `while` loop's candidate bounds and a
recursion's measures become obligations kept apart (`Vc.term_obs`,
`term_query`, `measure_check`), asked together. Its results decide
`Methods.seen` (`termination.seen_through`) and `Methods.unfold`, and are
cached (`loops_file`, `measure_file`).

```strata smt.call
```

```strata smt.def_call
```

```strata smt.congruent
```

```strata smt.share
```

```strata smt.assert_by
```

```strata smt.while_loop
```

# From obligations to scripts
This page describes main as it is. The P1, P3 and P5 agents are changing
the batch builder now: building each batch once, a faster `str_find` and
`headed`, and facts kept as ids instead of text.

`batch(vc)` turns a function's obligations into one script. Its state is
three text-keyed dicts, shared across the function's obligations:
`known` (fact text to its unknowns), `cache` (asserted line to its `Piece`)
and `named` (shared line to its name). For each obligation:

1. **`cone_in`** concatenates `defs`, the facts from `first_fact` to
   `facts`, `own` and `extra` into one list. It seeds a set with the
   unknowns of the goal, the path and the parameters' leaves (`consts_in`,
   a word scan of each term). It then passes over every fact until none is
   added, keeping each fact that shares an unknown with the set. A fact's
   unknowns are scanned once per batch (`known`), but the passes cost
   passes × facts × unknowns for every obligation.
2. **`body_in`** declares the unknowns kept, writes one `(assert …)` line
   per fact, path term and the negated goal, and reads each line through
   `piece`. `piece` finds the string terms in it by words (`keys_in`) and
   by a scan for `(` (`found_terms`), and replaces them by `str{i}`
   (`replace_terms`). Then come the `str{i}` equations, the length bounds,
   `check-sat` and a `get-value` of the leaves, all as one `String`.
3. **`shared_lines`** splits that text on `\n` again. A `declare-` line
   goes before the `push`, once per batch. An `assert` longer than 120
   bytes becomes `(define-fun |fact!N| () Bool …)` once, and the obligation
   asserts its name.
4. The obligation is wrapped: `push`, a comment with its name, its lines,
   `get-info :reason-unknown`, `get-info :rlimit`, `pop`,
   `echo "@@answered"`.

**`headed`** puts the header first (`header`: options, `ALL`, `utf8_len`,
`declarations`), then each `fun_defs` definition the script names. It
finds that with one `contains` over the whole script per definition.

**Limits.** A batch runs with `-t:20000 rlimit=2000000`: a 20 s soft limit
and 2M resource units per query. `read_replies` matches each reply to its
command (`:print-success`), splits at the `@@answered` echoes, and turns
Z3's running `:rlimit` into each query's own units.

**`answers_all`** runs every function's batch together (`run_scripts`).
Each obligation a batch didn't reach is asked alone (`single`, `-T:20`, a
hard 20 s). An obligation the batch answered `unknown` with `canceled` is
asked again alone (`retried`): Z3 inside `push`/`pop` skips simplifications
it makes on a script of its own. The second answer wins when it is proven,
or when the batch's timed out and it didn't. A model that can't be shown is
asked again with the lists' first elements (`example_facts`,
`example_leaves`). `answer_of` reads each reply: `unsat` is proven; `sat`
gives a model read into code (`literal_of`); anything else says why
(`gave_up`). "Timed out" (time ran out before the units did, or `-T`) is not
cached. "Gave up" (units ran out) is cached, since the limit is
deterministic. Last, `unmixed` calls `sort_mixes`, which builds the batch a
third time and parses it with `sorts.sort_error`.

```strata smt.batch
```

```strata smt.cone_in
```

```strata smt.body_in
```

```strata smt.shared_lines
```

```strata smt.headed
```

```strata smt.answers_all
```

# The solver pool and the cache
**The pool** (`c/solver.c`) keeps one `z3 -in` per core, each served by a
worker thread from one job queue. A job is `FLAGS 0x1f SCRIPT`, and jobs are
joined by 0x1e (`solver_pool_run`). Each script runs after `(reset)` with
`:timeout` and `:rlimit` set again, between two `echo` marks. `-T` is
enforced by `poll` and kills the process, which starts again for the next
job. A script that sets a global option (`(set-option :smt.…`) marks its
process dirty, and it is restarted before the next job. Starting Z3 costs
about 8 ms.

The pool's **memo** keeps every answer finished in this process. Its key is
two 64-bit hashes of flags and script plus the length, computed over every
byte of the script. A job equal to one running now waits for it
(`running_has`). `solver_pool_prefetch` queues jobs whose answers are only
kept: the worker queues each batch as soon as its function is encoded, and
`answers_all` later finds it by sending the same bytes again. Searches are
flagged `budget` and draw on one run-wide unit budget (`solver_budget`).
`STRATA_SOLVER_DUMP` and `STRATA_SOLVER_LOG` record what it was asked.
Without a pool (another solver, or a test's stand-in), `run_scripts` writes
each script to `scratch/HASH.smt2` (`text_hash`, a byte loop in Strata) and
runs them with `xargs -P`.

**Encoding with the pool.** `encode_solving` encodes every function on a
worker thread (`parallel.map`). With `Vc.live` set, `ask` sends each
question as it comes and waits for the answer. Without a pool,
`encode_rounds` reruns every function until its pending questions are
answered, one round of questions at a time.

**The verdict cache** (`verify-strata/checks/`, `verdict_file`) is keyed by
the settings (seed, solver path and version, std's fingerprint, package
settings), the label, the hash of the definition with everything it reaches
(`closure_keys`), the fuzz case count and the cross-check count. A hit is
the verdict, nothing run.

**The proof cache** (`verify-strata/proofs/`, `proof_file`) is keyed by
what the prover saw (`interface_keys`): a callee's interface hash when it is
seen only through its contract, its code hash when it is seen through. A
callee's body can change without changing its callers' proofs. A kept proof
is cross-checked again (`cross_checked`: 20 runs by default, of compiled
code), since the callee's code did change. A failure there is reported as a
prover bug (`prover_bug`).

**`check_fn`** layers the rest: a refuted check's model is replayed in
compiled code to confirm it (`replay`), the unproven checks are fuzzed, and
proven code shown to terminate is cross-checked. `write_verdict` skips
answers that may change (timed out, rejected, mixed sorts).

```strata smt.encode_solving
```

```strata smt.ask
```

```strata smt.run_scripts
```

```strata cross_checked
```

```strata check_fn
```

```strata write_verdict
```

# Where the time goes, and what term trees would change
**Measured** (IDEAS.md, the `tasks-proofs` entries, on `examples/tasks`):

- tasks-proofs-3: the dumped queries were 71 MB, and 59 MB after the
  congruence fix. `verify: encode` was 23 s of `memory.*`'s 34 s, spent in
  linear text scans of each batch (`command_heads`, `keys_in`, `words`,
  `str_len_terms`, `found_terms`, `replace_terms`). The solver took 3 s.
- tasks-proofs-4: reading each line once per batch (`cache`, `piece`)
  brought user CPU from 286 s to 213 s. Wall clock didn't improve.
- tasks-proofs-5: `claim`'s batch was 51 MB for 151 obligations, of 2.4 MB
  distinct lines. `shared_lines` took peak RSS from 3.85 GB to 2.0 GB and
  solver time from 85 s to 22 s. Encode was still 129 s.
- tasks-proofs-6: `claim` alone, encode 59.5 s of 63 s: the cone 25 s, the
  pieces 9 s, `shared_lines` 5 s, the declarations 0.1 s. With unknowns read
  once per batch: encode 41 s. An unknown-to-facts index built per
  obligation with copying appends took 176 s.
- tasks-proofs-7 and 8: proofs reshaped, then every definition proven. The
  package takes 52 s with `--no-cache`.

The scratchpad analysis (`verify-speed-ideas.md`) splits a 52 s cold run:
encode 37.1 s, LLVM for the drivers 6.5 s, cross-check and fuzz 6.1 s
(serial), fixed costs 1.5 s, and Z3 24-50 s of CPU over 243 scripts and
120 MB of text, the largest batch 10.6 s. Its findings: every batch is built
twice (`encode_answered` prefetches it, `answers_all` builds it again on one
thread to hit the memo), and on main a third time in `sort_mixes`. Batch
cost scales with the text before sharing, about 20 times the distinct
text. `headed` scans the whole script once per definition. A big
function's batch is one serial Z3 process.

**What a term-tree design changes, step by step:**

| step | today (text) | with term trees |
|---|---|---|
| making a term | `"(" + op + " " + a + " " + b + ")"`, a new string each time | a node of child ids, hash-consed: equal terms are one id |
| unknowns of a fact | re-scanned from its words (`consts_in`), cached by the fact's text | known when the node is built, as ids |
| dedup (`def_set`, `memo`, `share`, `path_term`) | keys are whole term texts, hashed at each lookup | keys are ids; `share` is free, since the same term is already one node |
| cone | fixpoint over all facts per obligation | a graph walk from the goal's ids over an index built once |
| placeholders `@@i`, `let_term`, `str{i}` | found and replaced by scanning text | substitution on nodes; string selectors named once when built |
| batch text | built per obligation, split and re-hashed by `shared_lines` | printed once per distinct node; a fact used by many obligations is one `define-fun` by construction |
| `headed` | `contains` over the script per definition | the union of the definitions each kept node uses |
| `sorts.sort_error` | parses the whole batch back into `Sx` | sorts checked as nodes are built, or skipped |
| solver input | text, hashed again by the pool's memo | still text (Z3 reads SMT-LIB); printing is the last step, and a node id or hash can key a per-query cache |
| replies, models | parsed from text (`read_replies`, `parse_sx`) | unchanged: Z3 answers in text |

What stays the same: the symbolic run's logic (paths, facts, obligations,
contracts, congruence, loops), the queries Z3 sees (they must stay
byte-for-byte comparable for the verdicts to stay put), the pool and both
caches. A term-tree encoder is judged by whether the scripts it prints match
today's, and by encode time on `claim` and on the tasks package.

```strata-refs smt.batch
```
