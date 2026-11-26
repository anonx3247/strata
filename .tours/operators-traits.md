---
title: Operators are trait methods
packages: compiler core
---

# Why
REBUILD.md rule 7 wants a stored tree made only of core forms, and §5b item 8
asks the lab to try operators as trait methods first: `a + b` is
`Add.add(a, b)`, and a type gets `+` by implementing `Add`, like any trait.
No special methods. Built-in arithmetic must compile and prove exactly as
before.

# Background
Until now `a + b` was a `Binary` node from the parser to the code generator.
The checker typed it as number arithmetic, string or list joining, and `==`
as a structural comparison on any type; `<` only on numbers and strings.

```strata check.binary
```

The printer shows a `Binary` with its precedence and line breaking:

```strata text.prec
```

# Intuition
`Vec2 { x: 1, y: 2 } + b` is stored as `Add.add(a, b)`, a call whose
callee is the reserved name `Add.add`. `view` shows `a + b`. `desugar` turns
the call back into `Binary { op: Add }`, so on `Int` the code generator emits
the same add-with-overflow-check it always did. On a `Vec2` with `impl Add
for Vec2`, the checker sees that the left operand's type has its own impl and
writes `a.add(b)`: from then on it is an ordinary method call.

# The traits
`core/ops.strata` holds the eight traits and core's impls for the built-in
types, whose bodies are the operators themselves.

```strata-diff ops.Add ops.Ord ops.Eq
```

```strata-also
ops.Sub ops.Mul ops.Div ops.Rem ops.Neg: the other traits
ops.impl*: core's impls for the built-in types, each body the operator
```

```strata-also
core_files shared_at pack_command: core.ops read with the other code modules
check.code_module codegen.shown_name graph.ensures_text smt.shown_name: core.ops is one of core's global code modules
```

# Stored as calls, shown as operators
`operators` is the one place that knows the mapping. The parser builds the
calls, the encoder writes any operator node it is given as its call, the
printer lowers each node before it prints it, and the resolver leaves the
reserved callee names alone.

```strata-diff operators.lowered_node operators.applied parse.binary binary.put_expr
```

```strata-also
parse.unary parse.statement: `-x` and `x += e` as calls
text.expr_text text.inline text.prec text.brace_on_spine text.bitwise text.stmt_text: each node lowered before it is printed
resolve.call: the callee `Add.add` is never resolved
operators.*: the rest of the mapping (`lowered`, `called`, the trait of each operator)
```

# The passes read calls
Nothing turns the calls back into nodes: `desugar` leaves them. The passes
that reason about arithmetic look at a call through `lowered_node`, a view
of `Add.add(a, b)` or `i32_add(a, b)` as the operator, and keep their logic.

```strata-diff operators.lowered_node check.ints_of smt.eval_expr codegen.expr_node
```

```strata-also
check.int_value check.float_value check.real_cuts check.real_holds check.as_listed check.unused_values: the checker's views
smt.bounds smt.comparisons smt.invariant_candidates smt.cannot_fail smt.added_to smt.amount smt.assigned_step smt.pass_walk smt.stable smt.literal_value smt.loop_facts smt.defining_ensures smt.visited_candidates smt.assume_visited smt.for_list_name: the prover's views
codegen.checked codegen.konst codegen.of_pred codegen.appended codegen.root_code: the code generator's views
codegen.escapes unique.ex roots.assign roots.flow roots.region_literal roots.strings: ownership: `x + 1` doesn't let `x` escape
codegen.wait_edges: an operator is no call that may wait (no cancellation point)
capabilities.constant capabilities.noalloc_call capabilities.runtime_call_errors: an operator is no function call for these rules
contracts.shallow contracts.konst contracts.holds derive.data_in exhaustive.scrutinee_type graph.flow_of termination.candidate_measures termination.cannot_fail eval.eval eval.holds desugar.elements_assigned: views
ast.calc_steps ast.calc_fact ast.step_node: `calc` steps written with operators
codegen.short eval.short: messages keep `Ord.lt`
desugar.mock_of graph.plan is_literal entry_expr write_found finding.quoted_place store.ex: views on stored trees (an operator depends on its trait's method in `store.ex`)
walk.decl_parts: a derivable trait's `requires` is walked too
```

# Intrinsics
Core's impls for the built-in types call intrinsics; a call of one is its
operator, compiled inline and encoded by the prover. Generic code calling
`x.add(y)` on a built-in type gets the operator inline.

```strata-diff operators.intrinsic_op codegen.builtin_operator codegen.method_call
```

```strata-also
ops.integer_* ops.float_* ops.i8_* ops.i16_* ops.i32_* ops.i64_* ops.u8_* ops.u16_* ops.u32_* ops.u64_* ops.string_* ops.bool_eq* ops.list_add*: the intrinsics
operators.is_neg_intrinsic operators.shown_node operators.operator_method operators.is_operator_name: their names
```

# Operands of two types
`impl Mul[Int, Money] for Money` becomes the function `$Mul$Money$Int`;
the checker calls it where the operands' types are that pair.

```strata-diff desugar.mixed_lowered check.mixed_operator check.binary
```

```strata-also
desugar.desugar_more desugar.last_part operators.is_operator_trait operators.mixed_name: the desugaring
check.definition_errors: one such impl per pair, beside the one for `Self`
select_names with_mixed: verify selects the function when it selects the impl
```

# Operators on the program's own types
`operators_resolved` runs before checking, only when the program implements
an operator trait for a type of its own or bounds a type parameter by one.
It infers each function using an operator and writes the operator on such a
type as its method's call.

```strata-diff check.operators_resolved check.own_operator
```

```strata-also
check.Inf check.infer_untagged check.own_negation: the operators met on such types, recorded while inferring
check.op_fn check.ops_called check.operator_traits check.traited check.built_in_head: the rewrite, in bodies and contracts
check.spreads_resolved: runs it first
```

# Proving through an impl's contract
`double(m) = m + m` is proven from `Money.add`'s `ensures`: a trait method's
call on a receiver of known type now uses the impl's `ensures` too, for any
trait.

```strata-diff smt.with_impl_contract smt.method_call
```

# Routine
```strata-routine
*.use* use*: imports
fault.* float.* int.* list.* mem.* run.*: core re-encoded by `strata upgrade`, which writes core's files without dependency lists
```
