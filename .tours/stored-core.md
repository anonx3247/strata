---
title: Handlers stored as matches
packages: compiler std core tools/check
---

# Why
REBUILD.md rule 7: a `.strata` file holds only core forms, sugar lives only in
text. This PR does it for the most used sugar, the handlers `x!`, `x?`,
`x except e { … }`, `x or { … }`, `x !! v` and `x ?? v`: the parser stores the
`match` each stands for, the printer shows it back, and the tree no longer has
`Try`, `Except` or `Fallback` nodes. Every `.strata` file is re-encoded.

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
