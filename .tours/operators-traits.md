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

# Back to nodes for the passes
`desugar` lowers the calls, so check, smt, codegen and termination see the
`Binary` nodes they always did. The few places that read stored trees
directly lower them first.

```strata-diff desugar.unsugared_node builtins.core_of
```

```strata-also
desugar.mock_of: models' `ensures` matched through the lowered node
graph.plan: state graphs read lowered trees
verified_decls: core's refinements lowered for the prover
entry_expr is_literal write_found: the invariant search reads and compares lowered trees
finding.quoted_place: an error quoting `+` points at the call
store.ex: an operator depends on its trait's method, as a method call does
walk.decl_parts: a derivable trait's `requires` is desugared too
```

# Operators on the program's own types
`operators_resolved` runs before checking, only when the program implements
an operator trait for a type of its own or bounds a type parameter by one.
It infers each function using an operator and writes the operator on such a
type as its method's call.

```strata-diff check.operators_resolved check.own_operator check.binary
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
