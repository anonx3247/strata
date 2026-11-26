---
title: The stored tree is the core language
packages: compiler std core tools/check
---

# Why
REBUILD.md rule 7: a `.strata` file holds only core forms, sugar lives only in
text. The parser stores the handlers (`x!`, `x?`, `except`, `or`, `!!`, `??`)
as the `match` each stands for, and ranges, elements, slices, element
assignments, comprehensions, `Dict`/`Set` literals and `loop` as calls of
builtins and `while true`; the printer shows them back. The tree loses seven
node kinds, the passes after parsing lose 893 lines of arms for them (323
lines now recognise a core form's shape for a fast path or a message), and
`desugar` loses the part that did this after reading. `check compiler`: 5.2 s
→ 4.7 s (noisy), memory +16%. Every `.strata` file is re-encoded, and the
round-trip law holds on all 125.

# Background
The tree had three nodes, and every pass had arms for them: walkers took their
children, the checker typed them, codegen compiled them with fast paths
(`try_want`, `except_expr`), the prover encoded them (`try_value` now,
`fallback_value`).

```strata codegen.except_expr
```

```strata smt.fallback_value
```

# Intuition
`n := parse(s)!` is stored as

`n := match parse(s) { Ok($v) => $v, Err($e) => return Err($e) }`

No program can write `$v`, so a match of exactly that shape is always a
handler: `sugar.handler_of` reads it back, and `view` prints `parse(s)!`. A
match written out with ordinary names stays a match.

```strata-refs sugar.handler_of
```

# The core forms and their reading back
`handler_match` builds the match; `handler_of` recognises it, through
`result_handler` and `option_handler`. `handler_match` ensures a handler the
parser makes is read back as itself (`canonical`).

```strata-diff sugar.Handler sugar.handler_match sugar.handler_of
```

```strata-also
sugar.result_handler sugar.option_handler sugar.canonical sugar.handler_parts: reading a match back as its handler
sugar.use*: the module's import
ast.Expr: the three nodes removed
```

# Parser and printer
The parser makes matches where it made the nodes; the printer prints a
handler match as the handler, with the same precedence and layout rules.

```strata-diff parse.postfix_from text.inline text.handler_text
```

```strata-also
parse.binary: `??` and `!!`
text.prec text.short_fallback text.brace_on_spine text.handler_on_spine: precedence and layout of a handler (and `(-1).abs()` kept in parentheses)
text.shorthand: replaced by `short_fallback`
parse.use* text.use*: imports
```

# Calls of builtins
`a..b` is `range(a, b)`, `xs[i]` is `at(xs, i)`, `xs[a..]` is `slice(xs, a,
len(xs))`, `xs[i] = v` is `xs = set_at(xs, i, v)`, `[x * 2 for x in xs]` is
`map(xs, x -> x * 2)`, `Dict { "a": 1 }` is `dict([("a", 1)])`. Each builder
has a reader the printer uses (`range_parts`, `slice_parts`, …). A module
can't name a function `at` or `map`, so the call is always the builtin.

```strata-diff sugar.slice_of sugar.element_assigned sugar.comprehension text.sugared_text
```

```strata-also
sugar.*: the other builders and readers
parse.index parse.statement parse.expr parse.sym_primary parse.collection_body parse.comp_fors parse.assignable: the parser builds them
text.stmt_text text.block_text text.expr_text text.clauses text.indexed text.while_head: the printer shows them
capabilities.capability_errors_in: a function named after a builtin sugar calls is refused
desugar.desugar_more desugar.unsugared: only compile-time `Dict`/`Set` values are left to lower
binary.expr binary.stmt: older files' nodes read as their core forms
```

# A hand-written match is a handler
`match r { Ok(v) => v, Err(e) => return Err(e) }` is stored as `r!`
(`canonical`): the same program, shown as the handler, typed by the same
rules.

```strata-diff sugar.canonical parse.ident_primary
```

# Passes that need more than a match
Where a handler means more than its match, the pass recognises it and does
what it did: the checker's messages and exact error type, codegen's fast
paths, the prover's encoding.

```strata-diff check.infer_untagged check.infer_handler codegen.expr_node
```

```strata-also
check.narrowed_in: a handler narrows nothing (else `o ?? "s"` typed `o` as unknown)
codegen.expr_want_node codegen.result_kind codegen.escapes codegen.sub_exprs: as before for handlers
smt.eval_expr smt.try_value: the same encoding
capabilities.walk capabilities.allocation capabilities.noalloc_sites: `!` in a task, noalloc sites
derive.expr derive.data_in: `comptime v.name ?? ""` read at compile time, not a type match
exhaustive.seen_in: a handler's arms cover its value
graph.block_flow graph.flow_of graph.stmt_flow: a handler is a value (its arms as branches made `view --graph` exponential)
```

# Removed sugar handling
```strata-routine
walk.* ast.bare check.errors_in check.children capabilities.children declarations.children eval.eval eval.parts locate.* mutability.walk pending.visit resolve.expr roots.flow store.ex unique.ex termination.* smt.names_bound smt.pass_walk smt.cannot_fail codegen.locals_of codegen.root_code graph.returns_early: the arms for `Try`, `Except`, `Fallback` removed: a match covers them
binary.expr binary.put_expr: the three tags no longer read or written
*.use*: imports of the removed nodes
```

# Re-encoded files
```strata-routine
*: re-encoded (handlers as matches); docs rewrapped and one-expression blocks stored as their view reads back (`tools/roundtrip.sh --fix`)
```
