---
title: Exact rationals, `Real`, the type of decimal literals
packages: compiler core std
---

# Why
`0.1 + 0.2 == 0.3` was false, and `verify` modelled no `Float` code. A decimal
literal is now an exact rational, `Real`, unless a `Float` is wanted. The prover
encodes `Real` as SMT's `Real` sort, so `0.1 + 0.2 == 0.3` and
`min(a, b) <= (a + b) / 2.0` are proven. `sqrt`, `exp` and the like stay on
`Float`. Measured cost: adding `0.1` 10^6 times takes 60 ms as `Real` and 1 ms as
`Float`.

# Background
Each backend works out the types of expressions on its own: codegen from the
values it compiles, smt from its terms. Only `check` knows the expected type at
each point. Codegen and smt run the checker's inference over every body first.
`check.typed` does this for codegen and `lambdas_typed` for smt, and both write
what it found back into the tree:

```strata check.typed_one
```

# Intuition
`mut t := 0.0; for s in shapes { t = t + s.area() }`: the literal starts as a
type variable. `t + s.area()` unifies it with `Float`, so the literal is a
`Float`. In `x := 0.1; print(x + 0.2)` nothing makes it a `Float`, so both are
`Real`s. The checker records each literal's type under a mark, and the
literal is written back for the backends either as a `Float` or as
`Real { num: 1, den: 10 }`.

# The type and its arithmetic (core.real)
`Real` is a record kept in lowest terms. That makes equality, hashing and
`Dict` keys structural. Its operations are written in Strata over `Int`.

```strata-diff real.Real real.real_add real.real_of
```

```strata-also
real.real_sub real.real_neg real.real_mul real.real_div real.real_mod real.real_cmp: the operators
real.impl Real: from, abs, min, max, floor, ceil, round, to_float, parse
real.real_to_float real.real_bits real.real_scaled real.real_rounded: nearest Float, ties to even
real.real_text real.real_fixed real.real_pow10 real.impl Display for Real real.impl Debug for Real: `0.1`, `1/3`, `{x:.2}`
real.real_parse real.real_exponent_at real.real_digits real.real_sum real.real_gcd
builtins.*: abs/min/max/floor/ceil/round/to_float/sum on Real
```

# Decimal literals in the checker
`dec_literal` gives a literal its type. It is a `Float` or a `Real` when one is
asked for. With no type asked for, it is a type variable of `Inf.decs`.
`default_dec` makes that variable a `Real` where a method or an overloaded
builtin needs a concrete type, and `dec_errors` reports one that became
anything else. `dec_marked` and `dec_unmarked` carry the inferred types to the
backends.

```strata-diff check.dec_literal check.dec_unmarked check.binary
```

```strata-also
check.Inf check.fn_inferred check.default_dec check.defaults_dec check.dec_errors: type variables of literals
check.infer_untagged check.infer_if check.numeric check.builtin_call check.fitting_builtins check.infer_method check.float_wanted check.is_num_lit check.is_dec_lit check.ends_in_num_lit check.real_ty check.real_lit check.int_expr check.dec_marked check.dec_mark: the literal rules
check.typed_one check.fn_lambdas_typed check.instantiated: marking for codegen, smt, eval
check.infer_call: a marked literal as a builtin's first argument is not a call
check.fn_types: `extern c` functions' parameter types (`scale(1.5)` passes a Float)
check.check_spec check.allocating_arith: `.N` and noalloc on Real
text.decimal_fraction text.decimal_pow10 text.decimal_gcd: a Float literal's shortest digits as a fraction
```

# Codegen
An operator on two `Real`s calls core.real. A literal that no pass typed,
in a contract or a refinement, becomes a `Real` beside a `Real`. A `Real`
literal beside a `Float` becomes a `Float`. The fuzzer generates a `Real` as
an `Int` over a `Positive`, reduced.

```strata-diff codegen.real_op codegen.apply codegen.produce
```

```strata-also
codegen.as_type codegen.binary codegen.real_lit_of codegen.hex_value codegen.is_real: literal fallbacks
codegen.expr_node codegen.format_spec codegen.builtin_call codegen.cmp_helper: `-x`, `{x:.2}`, `sum`, `sort`
```

# verify
`VReal` is a term of SMT's `Real` sort. `/` and `%` require a nonzero divisor,
as on `Int`. A model's real becomes a `Real` literal for counterexamples.

```strata-diff smt.real_binary smt.real_fn smt.binary
```

```strata-also
smt.Val smt.shape smt.sort_of smt.from_term smt.term_of smt.eq smt.eval_expr smt.builtin smt.call_exprs smt.sx_literal: the sort through the encoder
smt.real_term smt.real_const smt.int_of_lit smt.real_literal smt.real_parts smt.real_gcd
```

# Speed
The first build was 45 times slower than `Float` on a loop adding `0.1`.
Each change below was measured on its own (IDEAS.md has the table):

- A literal is now a constant block, so it allocates nothing.
- `t = t + x` puts its words into `t`'s own block. A nested operator's
  result stays on the stack.
- core.real has no call-stack frames.
- `Int` `==` calls the runtime only when both sides are big.
- The GCD uses Euclid's algorithm on machine words.

```strata-diff codegen.real_constant codegen.held_apply codegen.equal real.real_gcd
```

```strata-also
codegen.binary codegen.flags_for codegen.assigned_call codegen.block_expr codegen.stack_let codegen.function: the flags through `binary`, no frames for `real_…`
real.real_add real.real_mul real.real_reduced: fast paths, a division only by a GCD above 1
```

# Integer literals, patterns, division by zero, private parts
In a function that names `Real` or holds a decimal literal, each integer
literal is marked too (`$int`), so `x * 2` gets a `Real` 2. A decimal
pattern parses as `where it == 0.5` and prints back as `0.5`. A division
checks its divisor unless `verify` proved it nonzero. A `Real` can't be
built from its parts.

```strata-diff check.int_literal check.real_context codegen.real_op check.real_errors
```

```strata-also
check.dec_marked check.int_mark check.int_inner check.real_in check.is_int_lit check.int_value check.dec_unmarked check.infer_untagged: integer marks
parse.pattern_one parse.dec_pattern text.pattern_text codegen.matches smt.pattern: literal patterns
check.infer_block main.check_errors_in: the parts are never assigned
codegen.as_type smt.real_term eval.real_pair: an integer constant beside a `Real`
```

# core.real proven, exact data
Inside core.real, smt sees a `Real` as its record (`real_record`), so its
own functions are proven. Everywhere else a `Real` is an SMT real, and a
part's refinement (`den: Positive`) is core.real's to keep. std.json's and
std.toml's `Number` now hold a `Real`, built in two bootstrap stages (A: core has `Real`, std the old `Number`; B: `Number(Real)`, seeded by A).

```strata-diff smt.real_record smt.refined real.Real
```

```strata-also
smt.real_key smt.shape smt.sort_of smt.from_term smt.check_type smt.check_type_at smt.in_type smt.shape_test smt.plain_real
std.json.* std.toml.* std.sqlite.* std.orm.*: Number(Real) beside Integer(Int), Json/Toml/Sql for Real (an error for 1/3), Row.real, TEXT columns
real.real_of real.real_mod real.real_text real.real_to_float real.real_pow10: guards and bounded loops for the proofs
```

# Routine
```strata-routine
main.core_files main.shared_at main.pack_command check.code_module codegen.shown_name smt.shown_name graph.ensures_text: core/real.strata listed with the other code modules
eval.eval eval.binary eval.compare eval.real_parts eval.real_value eval.real_binary: Real in the evaluator
std.json.impl std.json.Json for Real: whole numbers as integers, others as the nearest Float
check.atoms eval.builtin: `mut x: Float = 0.0` where a Float accumulator was meant
```
