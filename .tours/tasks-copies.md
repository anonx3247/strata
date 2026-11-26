---
title: Tasks get copies, no atomic counts
packages: compiler core
---

# Why
REBUILD.md's rule: no object is ever counted from two threads. Until now a
value used inside a `spawn` was marked shared (the sign bit of its count word)
and counted atomically while tasks ran. Now a task gets its own copy of what
it uses from outside, as an Erlang process does, so every count is a plain
load and store. Single-threaded `strata check` of the compiler: -1.8%
instructions, -8% cycles. A large value the parent keeps, crossing 80 spawns:
15x the instructions (each spawn copies it).

# Background
What crosses to another thread is fixed by the language: a `spawn`'s
captures, a value sent on a channel, a task's result when its handle was
sent elsewhere. Codegen walked such values with `share:T` helpers marking
each block; those helpers are where the copies now happen.

```strata codegen.crosses
```

# Intuition
`xs := [1, 2]; h := spawn len(xs); mut xs.push(3)`: the closure holds `xs`
twice (the parent's variable and the capture), so the task gets a copy of the
list and the parent then pushes in place on its own. Had `xs` not been used
after the spawn, the closure would hold it once and it would move: one test of
the count word, no copy. A count of 1 under an owner that gives it up means
nothing else holds the block, so moving needs no proof; below a copied block
everything is copied, since the original keeps its children.

# The cross helpers
`cross_value` replaces `share_value`: it returns the value that crosses.
Each helper starts with `cross_head` (0 or below: as it is; held once and
owned: moves; else core.rt's `strata_cross_block`), then crosses what the
block holds, owned or borrowed as the head found.

```strata-diff codegen.cross_value codegen.cross_fn codegen.cross_head rt.strata_cross_block
```

```strata-also
codegen.cross_fields codegen.set_word codegen.cross_buffer_fn: fields and list buffers
codegen.closure codegen.lambda: a closure's fourth word is its cross function, taking the owned flag
codegen.vtable codegen.dyn_cross codegen.dyn_wrapper codegen.mock_table: a trait table's last entry crosses the slot
rt.strata_cross_flat rt.block_bytes rt.heap_alloc rt.strata_heap_alloc rt.strata_alloc rt.strata_int_cross int.int_crossed: flat blocks, sizes, copies outside arenas
codegen.share_value codegen.share_fn codegen.share_held codegen.share_fields codegen.tail_share: the marking walk, removed
```

# Where values cross
The spawn, a send, a select's send and a channel op cross owned. `await`
copies, borrowed, only when the handle crossed; else the result gains a
holder as before.

```strata-diff codegen.spawn_expr codegen.await_one
```

```strata-also
codegen.select_expr codegen.channel_op: sends cross owned
codegen.declarations: the new runtime entries
codegen.equal: channel ends compare by channel (an end that crossed is a new end)
```

# Plain counts
With nothing shared, core.rt's counting loses its atomic paths, the shared
bit, the keep-shared epochs and kept counts. The inline counting in the IR
loses its runtime call for negative words. `strata_memory_keep_shared`, which
the compiler's `parallel.map` turns on, now freezes what crosses instead of
copying it (`frozen_mark`).

```strata-diff rt.strata_release_word rt.strata_memory_keep_shared rt.frozen_mark codegen.counting
```

```strata-also
rt.strata_release rt.strata_dup rt.strata_dup_word rt.strata_unique rt.count_bits rt.threaded rt.strata_memory_threaded rt.rt_state rt.block_room rt.strata_freezing: plain counts, freezing
rt.atomic_counts rt.shared_bit rt.held_bits rt.epoch_bits rt.kept_count rt.strata_memory_all_atomic: removed
rt.strata_share rt.strata_int_share int.int_share: older seeds' marks freeze
list.slots_taken: a frozen buffer is never pushed past its end
parallel.map parallel.driver_keep_shared: docs: the workers' captures freeze
```

```strata-routine
rt.test* rt.use* int.use* list.use*: tests and imports
run.given run.ct_kept fault.text_lit fault.text_count float.two_to float.fraction_of float.power_of float.nat_bits float.nat_bit float.nat_low_bits float.text_done float.word_mask int.big_base int.digit_mask int.num_count int.digit int.set_digit int.num_add int.int_add int.int_sub int.int_mul int.int_eq int.int_abs int.int_div int.int_mod int.bit_op int.int_and int.int_or int.int_xor int.word_mod list.list_buf list.list_off list.buf_used list.slot list.list_new mem.f64_from_bits* mem.rem_unsigned*: no change of theirs: `strata diff` reads a `noalloc` function of an unchanged module as changed
```
