---
title: std.array, NumPy-like arrays of floats and integers, with sized-int loops that compile tight
packages: std compiler
---

# Why
std had no numeric arrays. This PR adds `std.array` with three array types: `Array` (float64), `ArrayI64` and `ArrayI32`. They have views, broadcasting, reductions, `dot` and `matmul`, and shapes are contracts that `verify` proves at call sites.

There is no `Int` in the module: shapes, strides, offsets and indices are `I64`. Getting those loops to compile to plain machine code needed changes in core, codegen, the prover and the checker. Measured against C and NumPy (IDEAS.md, "std.array"): a sum of 10M float64 went from 9.4 ms (first version, `Int` indices) to 3.5 ms. C takes 2.0 ms and NumPy 1.4 ms. An int64 matmul 256 takes 12 ms, against NumPy's 9.8 ms.

# Background
A list is a header (length, buffer, offset) over 64-bit slots. `xs[i]` took an `Int`, and every `Int` `+` or `<` has a big-integer path that calls the runtime. A float operation's finiteness check called the runtime too, and that call came back into the loop. LLVM must assume such a call writes memory, so it reloaded the header on every element.

```strata codegen.finite_test
```

# Intuition
`for i: I64 in 0..n { s += xs[i] }` should be `ldr d2, [x11], #8; fadd; …`. That needs no call anywhere on the loop's cycle. So the index is an `I64` that reads the slot directly, conversions take their small case inline, and the code doesn't come back from a fault's call.

```strata-refs std.array.sum_run_f64
```

# A list indexed by an I64
Core gets a second signature of `at` (`core/builtins.strata`: `builtin fn at[A](xs: List[A], i: I64) -> A requires 0 <= i.to_int() && i.to_int() < len(xs)`). The checker picks it by the argument's type, and the prover reads its `requires` through `to_int`. The checker now also reports a call that fits no signature once the argument types are all known (`xs["a"]`), which a generic signature had hidden.

```strata-diff check.builtin_call check.settled
```

```strata-routine
check.basic: replaced by `settled`
```

# Codegen: no call left in the loop
`finite` ends its slow path with `unreachable` where the runtime never returns, or at the supervised task's cancel point. The conversions between `Int` and the sized types test the small case inline. `int_bits`'s test (`a + 2^62 < 2^63` unsigned) is one LLVM folds for a widened `I32`.

```strata-diff codegen.finite codegen.word_to_int codegen.conversion codegen.int_bits
```

```strata-also
codegen.as_type: uses `int_bits`
codegen.lit_here: the bounds of a range check, made and dropped in the slow block
codegen.declarations: `strata_finite` declared `cold`
```

# Prover and fuzzer: sized values stay sized
`mut i: I64 = 0` used to bind an `Int`, so `to_int` of every sized loop counter was "not modelled". Now an integer at a sized type is a value of that type: in a `let`, an assignment and an `if`. It compares with one, and a list's length is below 2^62, so `len(xs).to_i64()` is proven. The fuzzer generated a refined alias of a sized type (`Length = I64 where it >= 0`) as a plain `I64`, and then blamed the prover.

```strata-diff smt.eval_as smt.merge smt.eq codegen.produce
```

```strata-also
smt.assign smt.as_word_of: an integer assigned to a sized variable
smt.builtin: `len` below 2^62
```

# The arrays
`Length`, the float array, and the layout code the three types share. The integer types are the same template with wrapping arithmetic, and `ArrayI32`'s sums are 64-bit.

```strata-diff std.array.Array std.array.Length std.array.gather
```

```strata-also
std.array.ArrayI64 std.array.ArrayI32: the integer arrays
std.array.count std.array.row_major std.array.repeated std.array.broadcast_shape std.array.broadcasts std.array.without_axis std.array.in_bounds std.array.position std.array.stretched std.array.lanes std.array.inside std.array.floats std.array.as_float std.array.clamp std.array.max_i64 std.array.min_i64: layout helpers
std.array.full std.array.zeros std.array.ones std.array.arange std.array.from_list std.array.from_rows: float shorthands
```

# Sums and dot products
Four interleaved partial sums. The bounds are clamped up front (`clamp`), so no index arithmetic can wrap and every index is proven.

```strata-diff std.array.sum_run_f64 std.array.dot_run_f64
```

```strata-also
std.array.sum_run_i64 std.array.dot_run_i64 std.array.sum_run_i32 std.array.dot_run_i32: the integer copies
```

# List.grow
`mut xs.grow(n)` makes room for `n` more elements, and the list's value doesn't change (`ensures xs == old xs`). In core, a list held once grows its buffer where it can, and a shared one is copied with the room. Codegen does what `push_in_place` does. The prover, `eval`, the uniqueness analysis and roots each learn the name.

```strata-diff codegen.grow_in_place codegen.grow_size_check smt.builtin
```

```strata-also
codegen.update_in_place codegen.list_builtin codegen.is_list_builtin: `grow` routed
smt.changed builtins.changing builtins.changes_as: `mut xs.grow(n)` is `xs = grow(xs, n)`
unique.assign_flow unique.in_place_op roots.growing: an update in place
eval.builtin eval.builtins: at compile time, the list itself
```
