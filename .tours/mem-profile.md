---
title: Where memory goes, measured live
packages: compiler core
---

# Why
Removing atomic counts cut cycles but few instructions, and neither
tasks-copies (#564) nor regions (#568) moved peak memory. To find out why,
this measures five programs live: their memory curves, and what holds the
heap at its peak. The results and answers are in IDEAS.md, "Memory: where
memory goes, measured live".

# Background
`--memory-sites` numbers each counting operation in the IR as a site and
counts how often it runs (`counted_sites`). It says how much memory work
happens, not what holds memory at a given moment.

```strata codegen.site_kind
```

# Live objects by site
Each allocation records the site the thread last set; each free takes the
object off that site's live counts. When the live bytes pass the last peak
by 2%, the counts are copied as the peak's. Small objects keep their site
in a `U32` per 16 bytes of the region; large ones go in a table.

```strata-diff rt.strata_live_start rt.live_born rt.live_died rt.live_add codegen.counted_sites
```

```strata-also
rt.live_snapshot rt.large_put rt.large_find rt.large_grow rt.hash_slot: the peak's copy and the large objects' table
rt.strata_site_set rt.heap rt.rt_state: the thread's site, the tables' word, the locks
rt.strata_alloc rt.strata_free: the hooks, a load and a branch when off
```

```strata-routine
fault.text_count fault.text_lit float.fraction_of float.nat_bit float.nat_bits float.nat_low_bits float.power_of float.text_done float.two_to float.word_mask int.big_base int.bit_op int.digit int.digit_mask int.int_abs int.int_add int.int_and int.int_div int.int_eq int.int_mod int.int_mul int.int_or int.int_sub int.int_xor int.num_add int.num_count int.set_digit int.word_mod list.buf_used list.list_buf list.list_new list.list_off list.slot mem.f64_from_bits* mem.rem_unsigned* run.ct_kept run.given : no change of theirs: `strata diff` reads a `noalloc` function of an unchanged module as changed
```
