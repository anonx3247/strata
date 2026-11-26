---
title: References by stable id
packages: compiler
---

# Why
Every reference to a definition was stored by its qualified name. A rename rewrote every use, a diff guessed renames (same body, new name: a rename with a body change was a deletion and an addition), and a merge where one branch renames and another adds a use broke the new use. REBUILD.md §3b settles the format before the rebuild's L0: each definition gets an id when it is first stored, kept for good; references hold the id; each module file says which name each id has. The text form doesn't change.

# Background
A module file is a header (each definition's name, kind, hashes, dependencies, size) then the definitions, each with its own string table. In memory every pass works on the stored form, where a reference is a qualified name (`resolve.stored`); views shorten names (`resolve.written`). Edits rewrite only the definitions that changed (`codebase.module_bytes`).

```strata codebase.write_modules
```

# Intuition
`shop.strata` defines `area` (id `#7857…`) and `square`, which calls it; `main.strata` calls `shop.square` (id `#70f1…`). On disk `square`'s bytes hold `#7857…`, not `shop.area`, and `main.strata`'s header says `#70f1… = shop.square`. `strata rename shop.area rect_area` changes the name of `#7857…` in shop's header: `square`'s bytes stay as they are, and `main.strata` doesn't change at all. A branch that adds `shop.square(2)` while another renames `square` stores `#70f1…`, which reads as the new name after the merge.

# Ids in the tree and the format
`Decl.id` and `Variant.id` hold the id in memory (`0`: none yet). The header gets each entry's id, the module's variants' ids, and the ids of what it refers to in other modules with their names. A string table item is a text or an id; the decoder reads an id as its name, so every pass still sees qualified names.

```strata-diff ast.Decl ast.Variant binary.Header binary.Entry binary.read_header binary.table_items binary.own_name
```

```strata-also
binary.Reader binary.id_of binary.id_hex binary.named_ids binary.renamed binary.owned binary.read_named binary.read_with binary.module_with binary.entry_with binary.definition_at binary.decl binary.type_decl binary.variants binary.fn_decl binary.trait_decl binary.use_items binary.decode_interface binary.entry_strings binary.decode_module binary.decode_definition binary.read_checked binary.decode_module_checked: reading ids, and the definitions' own names short
```

# Writing by id
The encoder writes a definition twice: once named (every reference as its name), for the hashes and to collect the names it binds, then by id. A name that may refer to a definition is written as its id when one is known, unless it is bare and the definition binds a local of that name. The hashes stay those of the named form, so every cache keyed by them keeps its meaning.

```strata-diff binary.encode_module_kept binary.encode_stored binary.w_ref binary.w_own
```

```strata-also
binary.Writer binary.put_decl binary.put_fn binary.put_type binary.put_expr binary.put_pattern binary.put_stmt binary.put_select_arms binary.w_fields binary.w_bounds binary.w_params binary.w_fn_decls binary.w_str binary.w_id_ref binary.w_use_item binary.w_bind binary.w_binds binary.w_opt_bind binary.w_named_ids binary.w_id binary.id_to binary.flush binary.flush_header binary.parts_with binary.blank binary.implicit binary.encode_definition binary.encode_parts binary.encode_module binary.encode_bundle binary.entry_ids binary.raw_items binary.qualify binary.stored_bytes binary.ref_ids: writing ids and the header's tables
```

# Keeping ids
A definition without an id (parsed from text) takes the one of the definition it replaces: same module, name and kind, in the previous version or the stored file. New ones get 63 random bits. `rename` and `move` already spread `Decl`, so they keep ids.

```strata-diff binary.assign_ids binary.ids_of codebase.module_bytes codebase.write_modules
```

```strata-also
binary.variants_of binary.id_key binary.fresh binary.new_id binary.binary_new_id: new ids
codebase.encode_files codebase.encode_again script_bytes upgrade_command write_found store_check encode_at: callers that write whole files, with std's ids
resolve.union_members: a union member folded into a variant keeps the type's id
```

# Reading a package: the owner names an id
A file has its own copy of every name it refers to, so it reads alone. A package reads every header first and each reference as the module that has its id names it: after git merges a rename and a new use without the driver, the use shows the new name.

```strata-diff decoded_files
```

# diff and merge by id
`diff` adds the renames that ids show to the logged ones: a rename squashed with a body change, a move, a renamed variant. `merge-file` reads each id by one name on the three sides, so the old rename guessing is gone; two new definitions of one name are a name clash. After a move, `sync` only has to move imports.

```strata-diff diff.diff diff.id_renames merged_names merge.merge merge.follow_move
```

```strata-also
diff.named_ids: each definition's and variant's id
merge_file_command: sides read with one name per id; the result keeps the sides' ids
merge.renames merge.renamable merge.kind_of merge.words_of: the rename guessing, removed
header_command: `strata header` prints ids, variants and references
bad: a flipped id byte that no reference names isn't caught; trees compared without ids
same: compares a definition with its bytes as its header stores it
finding.interface_of edit_scope edit_errors: the header passed to read references
```
