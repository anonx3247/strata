---
title: std.array, NumPy-like arrays of floats
packages: std compiler
---

# Why
std had no numeric arrays. This PR adds `std.array`: an n-dimensional array of
`Float`s with views, broadcasting, reductions, `dot` and `matmul`. Shapes are
contracts, so `verify` proves or refutes them where the shapes are known.

It also measures the inner loops against C. On this machine, `strata build
--release`: a sum of 10M floats takes 9.4 ms (C: 2.0 ms for the same algorithm),
and a 256×256 matmul takes 36 ms (C: 7.6 ms for the same algorithm, 2.3 ms
vectorized). Before the codegen change on the last page, they took 19 ms and 97 ms.

# Background
A `List[Float]` is a header (its length, buffer and offset) over a buffer of
64-bit slots, and a float sits in its slot unboxed. `xs[i]` is checked against
`len(xs)` unless `verify` proved it. Every float operation's result must be
finite, and before this PR that test was a call into the C runtime:

```strata codegen.finite_test
```

# Intuition
A 2×3 array `arange(6).reshape([2, 3])` is the list `[0, 1, 2, 3, 4, 5]` with
shape `[2, 3]` and strides `[3, 1]`. Its transpose is the same list with shape
`[3, 2]` and strides `[1, 3]`, made in O(1). An operation first makes each operand
row-major (`to_list`: free for compact data, else one `gather`), then loops over
flat lists. Each index read is guarded by its loop's condition, so every bounds
check is proven.

```strata-refs std.array.gather
```

# The array and its views
The type, with `len(shape) == len(strides)` as its refinement, and the views that
share its data.

```strata-diff std.array.Array std.array.Matrix std.array.gather
```

```strata-also
std.array.count std.array.row_major std.array.dense std.array.repeated std.array.as_float: shapes, strides and filled lists
std.array.full std.array.zeros std.array.ones std.array.arange std.array.from_list std.array.from_rows: construction
std.array.in_bounds std.array.without_axis: shape predicates used in contracts
```

# Broadcasting and elementwise operations
NumPy's rule: lengths compared from the last axis, a 1 stretches. The stretched
axis gets stride 0, and both sides are gathered to the broadcast shape. Equal
shapes skip that. `add`, `sub`, `mul`, `div` and `neg` take `self` and another
`Array`, so `impl Add for Array` forwards to them once operators are trait methods.

```strata-diff std.array.broadcast_shape std.array.stretched
```

```strata-also
std.array.broadcasts: the predicate the operations require
```

# Reductions, dot and matmul
Sums add in four interleaved partial sums (`sum_run`, `dot_run`), so the additions
don't wait on each other. Reductions along an axis move the axis last (`lanes`)
and reduce each run. `matmul` makes one dot product per element, over a row of `a`
and a row of `b`'s transpose, so it never writes into the result list inside the
loop. Its `requires a.shape[1] == b.shape[0]` and `ensures it.shape == [a.shape[0],
b.shape[1]]` are what callers are checked against.

```strata-diff std.array.matmul std.array.dot_run std.array.sum_run
```

```strata-also
std.array.dot: the 1-axis dot product
```

# Float tests inline
`finite` now tests the result inline (`fabs` against infinity) and calls
`strata_finite` only on the failing path, which `declarations` marks `cold`. Before,
each float `+`/`*` was a call, followed in any program linked with std's library by
a cancellation poll. That alone halved the time of float loops.

```strata-diff codegen.finite codegen.declarations
```
