---
title: The runtime reached through extern c
packages: compiler core
---

# Why
REBUILD.md rule 8: the runtime is an ordinary C library, and compiled code
reaches it only through `extern c` declarations in core, each with `assumed`
contracts that are the trusted base under every proof. Before this PR,
codegen held a table of 203 hand-written `declare` lines and called runtime
functions by name everywhere, and nothing said what any of them does.

This PR is step 1: every runtime function is declared in core, codegen takes
the declarations from there, six string builtins are compiled through the
declaration instead of by name, `verify --assumed` lists what proofs trust,
and `verify` fuzzes the C against its contracts. The IR is the same as before.

# Background
Codegen lowers a builtin call by name (`string_builtin`'s cases) to
`call(g, ty, "strata_…", args)`, which marks the symbol used; at the end of
`generate` the used symbols' lines from `declarations` were appended.

```strata codegen.call
```

A program's `extern c` function is compiled as a wrapper whose body converts
the values to C (a `String` becomes a NUL-terminated copy):

```strata codegen.foreign_call
```

# Intuition
`byte_len("héllo")` was a case in codegen naming `strata_str_byte_len`. Now
core says it:

```strata runtime.strata_str_byte_len
```

and the builtin's body is that call, so codegen finds the C symbol in core
and emits the same `call i64 @strata_str_byte_len(ptr …)`.

# Declarations from core
`runtime_declarations` turns each core.runtime declaration into its IR line
(types as compiled code passes them; `assumed ensures false` is `noreturn`).
`generate` appends those and the few kept by hand, and fails on a symbol
neither declares.

```strata-diff codegen.runtime_declarations codegen.generate codegen.declarations
```

```strata-also
codegen.runtime_ll: a core.runtime type in IR
runtime.*: the 188 declarations, the strings group with contracts
core_files: core/runtime.strata read with core
```

# Builtins with a body
A `builtin fn` may have a body: one call of a core.runtime function with the
builtin's parameters as arguments. `builtin_links` collects them by name,
`linked` picks the overload by the arguments' types and emits the direct
call; `string_builtin` lost the six cases.

```strata-diff codegen.linked codegen.builtin_links codegen.builtin
```

```strata-also
codegen.Link codegen.body_call codegen.link_args codegen.runtime_symbols: the link table
codegen.link_errors codegen.new_gen codegen.Gen: a malformed body is an error
codegen.string_builtin: the moved cases removed
parse.decl_kind: a builtin's body is accepted
text.decl_body_text: and shown
builtins.*: trim, ord, chr, byte_at, byte_len, byte_slice get bodies
```

# What proofs trust, and fuzzing the C
`verify --assumed` lists each package's trusted clauses. `verify` on core
fuzzes core.runtime functions through wrappers with their contracts; a call
of a `strata_` extern compiles to the runtime call itself.

```strata-diff trusted_clauses runtime_wrappers checks_of
```

```strata-also
assumed_report clause_text chars_in_text verify_command: `--assumed`
runtime_wrapper_name verify_file: the wrappers added to what verify sees
codegen.foreign_call: a runtime extern called as it is
driver.usage_text: help for `--assumed`
```

# Routine
```strata-routine
int.* list.* float.* fault.* mem.* run.*: their files are untouched; the tour reports core's runtime modules as changed
```
