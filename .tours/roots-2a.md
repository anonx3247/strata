---
title: Statement regions (scoped allocation, phase 2a)
packages: compiler core
---

# Why
Counting frees each value by itself: an allocation, a release that reaches 0, a walk of the fields, a free. Phase 1 (`compiler/roots`, read only) found where each value could live if every scope had a region. This PR builds the region runtime and uses it for the first case: values that live only in the statement making them are bump-allocated in the thread's region and freed when the statement ends.

The measurement is the answer of this PR: on `strata check` of the compiler, 0.9% of the objects go in regions, the instructions grow 0.7%, peak memory is unchanged, and the micro-benchmarks put nothing in regions. Statement regions alone don't pay; destination passing (2b, planned in IDEAS.md) is where phase 1's share is. Building it found six rules of phase 1 that are unsound once values really go in regions.

# Background
Codegen compiles each statement in a scope of its own, whose temporaries are dropped at its end (`drop_scope`); early exits drop the scopes they leave. Arguments are borrowed, and a constructor passed to a function that doesn't keep it can already go on the stack.

```strata codegen.drop_scope
```

Phase 1's verdicts are keyed by function, the statement's text and the expression's text; codegen finds them by printing what it compiles.

```strata roots.walked
```

# Intuition
`keep_op(mut g, V { ty: i, op: f"%x.{i}" })` in a loop: the `V` lives only in the statement, so it can go in a region; but `keep_op` pushes `v.op` into `g`, so the f-string inside it outlives the statement and must stay counted. A region object may point to counted ones (its held list drops them at the reset); a counted one never points into a region.

```strata-refs codegen.region_site
```

# The run time: a region per thread
One arena-shaped block per thread, used as a stack of marks over the arenas' 1 MB chunks. Objects are tagged 8191 with a count of 2, so nothing frees, grows or changes one in place. Check mode never reuses region memory and fills it with freed marks.

```strata-diff rt.strata_region_mark rt.strata_region_held rt.strata_region_free
```

```strata-also
rt.strata_region_desc rt.strata_region_alloc rt.region_tag rt.poison rt.heap: the thread's region and its objects
rt.strata_region_concat: a string's + made in the region
rt.strata_region_thread_end: a task's thread gives its region back
rt.strata_arena_begin: arenas' tags stop at 8190
```

# Which values: roots' keys for codegen
A value goes in a region when its home is the statement it is made in, codegen can region its kind, and the function spawns no task.

```strata-diff roots.verdicts roots.in_own_statement roots.region_kind
```

```strata-also
roots.made roots.Flow roots.spawns roots.texty: recording the sites codegen can region
```

# Unsound rules found
Each was harmless while phase 1 only measured. A callee keeping a field of its argument keeps it (`parts_kept`, a fixed point over `leaks`); a String result may be an argument; copying built-ins may give back their operand; a one-part f-string is its part; a list's `+` shares elements unless a literal shows otherwise; other operators and unknown variables stay counted.

```strata-diff roots.parts_kept roots.leaks roots.flow
```

```strata-also
roots.part_of roots.mentions roots.literal roots.keeps_part roots.reads_only roots.Program: what a callee keeps
roots.holds_nothing roots.copies: String and the copying built-ins
roots.into_var roots.assign: unknown variables counted
roots.lists: removed, replaced by literals
```

# Codegen: marks, allocations, resets
A statement roots names opens a mark; its regioned values bump the top inline; its end (and any early exit or unwinding) resets inline, calling the held list's helper only when needed.

```strata-diff codegen.block_expr codegen.region_open codegen.region_reset codegen.region_site
```

```strata-also
codegen.region_alloc codegen.region_block codegen.region_count codegen.region_desc codegen.region_slot: allocation
codegen.region_reset_fn codegen.held_drop_fn: the held list's helpers
codegen.construct_want codegen.expr_node codegen.interp codegen.region_concat codegen.binary codegen.apply: the sites
codegen.expr codegen.expr_want codegen.function codegen.Gen codegen.Scope codegen.drop_scope codegen.unwind_all codegen.declarations: the flag, the scope's mark, unwinding
codegen.generate codegen_with run_command build_command: `--no-regions`
```

```strata-routine
float.* int.* list.* mem.* run.* fault.*: not changed by this PR (the files are byte-identical; `strata tour` reports their signatures as changed)
core_rt_text computed_value repl_eval: no regions for core.rt, compile-time code, the REPL
```
