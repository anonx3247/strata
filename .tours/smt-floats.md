---
title: Floats in the prover
base: 52cec923
packages: compiler, std
---

# Why
A `Float` is a double that is never NaN or infinite, so every `+`, `-`, `*`, `/` on floats is a check that its result is finite. The prover didn't model floats: each such check was "not proven", and in `std.array` 12 of 52 functions were only fuzzed because of it. This PR models floats in `compiler/smt` and gives `std.array`'s float functions contracts that make finiteness provable: all 67 of its functions are now proven.

# Background
Values the solver sees are `Val`s: an `Int` is a term (`VInt`), and before this change a `Float` was `Opaque`, so every float check was an unknown.

```strata smt.oblige
```

A check is an obligation on the path; past it, the path assumes it held.

# Intuition
`fn f(x: Float where abs(it) <= 1000000.0) -> Float = x * x`: the exact product `e = x·x` is at most 1e12, far below 2^1024 - 2^970, where a double's rounding goes to infinity. So the check "`*` gives a finite number" is `|e| < 2^1024 - 2^970`, exact, and the result `r` is a real within 2^-53·|e| of `e`. No floating-point theory is needed: the solver reasons about reals, and rounding is a bound.

# Floats as reals
`VReal` is a real term. Each operation is an SMT function of its operands (declared in `header`), named by a fresh constant, with the rounding bound, the sign (for `*` and `/`), the side of each operand's magnitude, and exact products with powers of two. Literals are their double's exact value.

```strata-diff smt.real_op smt.real_lit smt.float_builtin smt.Val
```

```strata-also
smt.arith smt.compare: float operands go to `real_op`, comparisons on reals
smt.eval_expr: literals and negation of floats; a type's own method in a contract (`own_function`)
smt.shape smt.from_term smt.sort_of smt.refresh: a Float is a `Real` within the largest double
smt.float_max smt.float_overflow smt.float_eps smt.float_tiny smt.float_range smt.rabs smt.real_num smt.power_of_two smt.as_real smt.is_real smt.float_functions smt.header: the constants and helpers of the encoding
smt.builtin smt.builtin_faults: `sqrt`, `floor`, `ceil`, `round` checks; a Float list's `sum` is a valid Float
smt.sx_float smt.literal_of smt.sx_literal smt.showable: counterexamples with floats
```

```strata-routine
smt.leaves_of smt.union_as smt.val_type smt.pairable smt.share_val smt.eq smt.subst_val_in smt.merge smt.val_key smt.type_key smt.term_of smt.pure_term smt.element_type smt.head_of smt.open_input: `VReal` alongside `VInt` and `VStr`
```

# What the prover can now carry
Three fixes the std.array contracts needed: a type's own method called in a contract (`self.within(b)`) is that method's function; a call made twice on the same values that gives a list is that same list, so an `all` known in a `requires` holds of the list the body reads; a function whose body is a literal is a constant.

```strata-diff smt.own_function smt.call_exprs smt.constant_body smt.same_call
```

# std.array: bounds on magnitudes
Each operation takes bounds on its arrays' elements (`within`), named by functions (`add_bound`, `factor_bound`, `sum_bound`, `dot_bound`, `product_bound`). The bounds reach the loops through refined element types (`AddSample`, `SumSample`…) on private helpers' parameters, and generic `gather` / `pair` keep them through a broadcast.

```strata-diff std.array.impl* std.array.add_lists std.array.sum_run std.array.dot_run
```

```strata-also
std.array.sub_lists std.array.mul_lists std.array.div_lists: the other element-wise operations
std.array.lane_sums std.array.transposed std.array.product: reductions along an axis, matmul's transposed copy, a bounded product
std.array.gather std.array.pair: generic, so the element type says what holds of a copy
std.array.add_bound std.array.factor_bound std.array.sum_bound std.array.dot_bound std.array.product_bound std.array.AddSample std.array.Factor std.array.Divisor std.array.SumSample std.array.DotSample: the bounds
std.array.dot std.array.matmul std.array.as_float std.array.Array: contracts and docs
std.sqlite.impl*: `Row.float` converts the 64-bit integers SQLite stores (a larger Int faulted in `float`)
```
