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

This PR declares every runtime function in core with the types compiled code
passes, gives 57 of the 196 declarations contracts, takes codegen's
declarations and its list of panicking functions from them, compiles eight
builtins through the declaration instead of by name, lists what proofs trust
(`verify --assumed`), and fuzzes the C against its contracts (50 functions).
The IR is the same as before.

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

# Contracts, and the C checking its own
Each declaration is typed with the values compiled code passes. A contract
calls another runtime function where it can, so fuzzing checks the C against
the C: `strata_int_add` ensures `it - b == a`.

```strata runtime.strata_int_add
```

A plain `requires` is one the C checks and panics outside; `panicking` takes
the set of functions that may panic from that, where codegen had a list.

```strata runtime.strata_div
```

```strata-diff codegen.panicking codegen.call codegen.after_builtin
```

```strata-also
codegen.soft_faults: the list, gone
codegen.Gen codegen.new_gen: the two sets
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
codegen.string_builtin codegen.number_builtin: the moved cases removed
parse.decl_kind: a builtin's body is accepted
text.decl_body_text: and shown
builtins.*: trim, ord, chr, byte_at, byte_len, byte_slice, float, sqrt get bodies
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
