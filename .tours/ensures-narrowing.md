---
title: What a function ensures of its result's variant narrows its calls
packages: compiler
---

# Why
A function that says which variant it gives (`ensures o is Paid ==> it is
Shipped`, `-> Order where it is Shipped`) should let its callers use that:
read `ship(o).tracking` in a `Paid` arm, or match only the variants left.
Refinements already worked this way through `verify` (`let n: Natural =
abs(y)` is proven at the call from `abs`'s contract). Variants didn't: the
checker typed any field of a sum as `TAny`, so `o.tracking` on any `Order`
checked, verified as "nothing to check", and read another variant's memory at
run time; and a `match` on a call of a generic function was never covered by
`strata check`.

# Background
The checker models a value narrowed by a `match` arm as `TVariants` (only for
sums without type parameters), and reads fields of one narrowed variant.
`exhaustive` covered matches on parameters and on calls of non-generic
functions, from the syntax tree alone.

```strata check.Ty
```

```strata check.arm_variants
```

# Intuition
`ship(o)` where `o` is narrowed to `Paid`: `ship`'s facts are `(it is
Shipped) == (o is Paid || o is Shipped)`; the argument's type decides the
condition (true), so the call is a `Shipped`. Where `o` is `Pending`, the
condition is false and the call is `Pending | Paid`. Where the condition is
`d.contains(k)`, the checker can't decide it: a match on the call that misses
`None` is left to `verify`, which proves some arm matches from the contract
and the `if d.contains(k)` around it.

# Narrowing a call
`types_of` keeps each function's facts about `it` (`walk.result_facts`, when
one says `it is V`). At each call `narrowed_at` intersects the variants the
facts leave, deciding conditions with `decide`; `narrowed_result` turns them
into `TVariants`. `infer_call` records the call for a `match` on it
(`Inf.last_call`).

```strata-diff check.narrowed_at check.narrow_by check.decide check.infer_call
```

```strata-also
check.Types check.types_of: each function's and method's parameters and facts about its result
check.narrowed_result check.conjuncts check.result_variants check.variants_named: the narrowed type, the facts' parts
walk.says_variant_of_it walk.tests_it walk.result_facts: which facts speak of the result's variant
check.Inf: the last call's variants, for a match on it
```

# Fields and matches
A field only some variants have is an error unless the value is narrowed to
them; a match on a call is covered against the variants it can give, and
`exhaustive` keeps only calls giving a `Bool`. `verify` obliges "some arm
matches" for a match on a call whose function speaks of its variant.

```strata-diff check.variants_covered check.variants_field smt.gives_said_variant
```

```strata-also
check.infer_untagged: a sum's field, or a narrowed value's, through variants_field
check.infer_match: covers a match on a call with the variants left
check.pattern_variants: the variants an arm takes whole
exhaustive.scrutinee_type exhaustive.Program exhaustive.match_errors: calls giving sums are the checker's now
smt.eval_expr: the coverage obligation for such calls
```

# `if x is V`, and where narrowing stops
`if o is Paid { o.amount }` narrows `o` in the branch taken. A variable or a
type variable never takes a narrowed type, since it may hold any variant
later.

```strata-diff check.infer_if check.is_narrowed check.widened
```

```strata-also
check.variable_is check.narrowed_env: the variable and its variants, the branch's types
check.unify: binds a type variable to the narrowed value's sum
check.infer_block: a `mut` variable gets the sum
```

# Methods
An `impl` method's facts (with its trait's) are kept under `T.m`, a trait's
under `Tr.m`; `infer_method` narrows as `infer_call` does. Each call takes a
number when it starts, so a match or a field read knows the record in
`last_call` is its own call's.

```strata-diff check.infer_method check.method_facts_key
```

# `Option` and `Result`
`TVariants` keeps the sum's type arguments, so a generic sum's arms and
`is` tests narrow it, and a narrowed value unifies as its sum.

```strata-diff check.Ty check.arm_variants check.sum_variants check.fits
```

```strata-also
check.bind_typed: a narrowed value's arguments for its arms' payloads
check.ty_text: a narrowed value written with its arguments
check.dropped_results: a narrowed `Result` thrown away still warns
check.variants_named: generic sums' variants too
```

# Field reads only verify decides
A read of a call whose facts left a condition open takes the field of the
variants left that have it; `verify` proves the result is one of them, and
the compiled code checks it until then.

```strata-diff smt.has_field codegen.has_field_check check.with_field
```

```strata-also
codegen.get_field: the check on a call's result
```

# A use
`expand_alias` never gives a `Refined`, said by its result type and proven;
`structured`'s match no longer needs a `Refined` arm.

```strata-diff smt.expand_alias smt.structured
```

# Routine
```strata-routine
check.use* smt.use* walk.use*: imports of the new helpers
```
