---
title: Structural records
packages: compiler
---

# Why
"Anything with `.id: OrderId`": a function over invoices and quotes alike
had to be written once per record type, or take the fields one by one.
Records are nominal, and the only structural tool was the spread `f(...x)`.
This PR adds a type meaning "any record with at least these fields",
`{ id: OrderId, total: Cents, .. }`, usable as a parameter's type and as a
type parameter's bound, checked, compiled, and verified.

# Background
Generic functions are already compiled once per type argument list: codegen
keeps a queue of instances (`score[A]`, `score[B]`) and substitutes the type
parameters. The checker knows a type parameter by its bounds, which so far
were only traits:

```strata check.bounds_by_param
```

`verify` proves a generic function with its type parameters as
uninterpreted sorts, so a field of a `T` was unknown, and the fuzzer runs a
generic function with `Int` (or a type implementing its traits):

```strata codegen.fuzz_instance
```

# Intuition
`fn due(x: { id: OrderId, total: Cents, .. }) -> Cents` called with an
`Invoice { id, total, paid }` and with a `Quote { id, total, days }`:

- the parameter's type becomes a type parameter of its own,
  `[$x: { id: OrderId, total: Cents, .. }]`, so everything after `desugar`
  sees an ordinary bounded generic;
- the checker reads `x.total` from the bound, and at each call checks that
  `Invoice` has `id` and `total`, with `total` written `Cents` too;
- codegen needs nothing new: `due[Invoice]` and `due[Quote]` read `total` at
  their own offsets;
- `verify` proves `due` once, with `$x` a record of exactly `id` and `total`.

```strata-refs check.row_met
```

# The type in the tree
A new type node, `RowType`, tag 5 at the end of the type tags (no file is
re-encoded). It is parsed in a type's place after a `{`, `..` required, and
printed back as written.

```strata-diff ast.TypeExpr parse.row_type parse.type_expr_plain binary.type_expr
```

```strata-also
binary.put_type: writes it
text.type_text: prints it
parse.fields: a record type written with `..` says where a structural type goes
walk.type_children walk.type_kids: walks it
resolve.ty resolve.named_type: names in its fields resolved, and back
binary.use* walk.use* declarations.use* derive.use* capabilities.use* contracts.use* desugar.use* diff.use* eval.use* parse.use* refactor.use* resolve.use* smt.use* store.use* termination.use* text.use* unused.use*: import `RowType`
```

# A parameter's type is a type parameter
`desugar` lifts each structural type in a parameter's type (at its top or
in its type arguments) into a type parameter named after the parameter,
bound by it. A structural type anywhere else (a result, a variable, a
field, an alias) is an error: it has no values of its own.

```strata-diff desugar.desugar_more desugar.fn_rows_lifted desugar.row_lifted desugar.row_errors
```

```strata-also
desugar.rows_lifted desugar.has_row desugar.bound_rows: helpers
declarations.signature_errors declarations.type_errors_in: a bound's fields' types checked, not taken for a trait
```

# Checking: fields read, calls met
Each function's structural bounds are kept by type parameter
(`Methods.rows`), and those in scope in the function being checked
(`Inf.trows`). A field read on such a type parameter is the bound's field,
or an error. A call checks each structural bound against the type the
argument gave the parameter: a record with each field of a fitting type
(the same written type for a refined field, since the callee is proven
knowing the refinement), or a caller's own type parameter whose bound has
the fields.

```strata-diff check.row_met check.field_met check.rows_by_param check.infer_untagged
```

```strata-also
check.Shape check.types_of: a record's fields' types as written
check.Methods check.methods_of: each function's structural bounds
check.Inf check.fn_inferred: the bounds in scope
check.infer_call: keeps the type variables of a function with structural bounds, checks each bound
check.infer_method check.static_call check.given_for: the same for a type's own methods, on a value and on the type
check.bounds_by_param: a structural bound is no trait
check.row_text check.row_refined: messages, refined field types
```

# Verifying and fuzzing
When `verify` proves a function itself, each structurally bound type
parameter is a record type holding exactly the bound's fields. The function
can only read those fields and makes no value of the type, so what holds
for that record holds for every record with them. Callers see the contract
through the type arguments, as for any generic function. The fuzzer
instantiates the type parameter with the program's first record that has
the fields.

```strata-diff smt.rows_as_records smt.function codegen.fitting_record codegen.fuzz_instance
```

# Every pass that walks a type
The rest is the new node in each exhaustive match over types: mapped like
a union's members where the pass rewrites types, its fields' types where it
collects or searches, opaque where a value would be modelled (a structural
type has none).

```strata-routine
ast.bare_type check.self_is check.mentions_any check.mentions_self check.type_head check.lower codegen.self_is codegen.written_in codegen.mentions_self codegen.mentions_any codegen.needs_check: the new node in a match over types
capabilities.json_error contracts.refined_errors declarations.type_names derive.subst derive.unwhere derive.ty derive.type_value_in derive.kind_of derive.mentions_name desugar.subst_type diff.type_head: the new node in a match over types
eval.type_head eval.fits eval.substitute eval.mentions_param eval.has_unknown locate.type_expr_kids locate.replaced_type refined_in takes_raw parse.type_head refactor.type_renamed resolve.type_head: the new node in a match over types
smt.structured smt.subst smt.shape smt.unfold smt.sort_of smt.sort_type smt.from_term smt.refined_by smt.refined_walk smt.erased smt.check_type_at smt.sx_ok smt.self_written smt.refined_types smt.sx_literal store.ty termination.refinements_in unused.type_names: the new node in a match over types
cache.Cache codebase.merge_parsed derive.pairs derive.remember desugar.inline_refinements diff.members_words docgen.titled graph.Shape merge.tokens resolve.walk resolve.union_members smt.infer_invariants termination.call_name termination.struct_call finding.untagged: stored again by the package-wide edit, the same text
```
