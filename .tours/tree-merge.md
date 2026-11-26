---
title: Diff and three-way merge on trees
packages: compiler
---

# Why
`strata merge-file` (git's driver for `.strata` files) merged a definition changed
on both sides by running `git merge-file` on its printed text. Two edits on
neighbouring lines, or to two arguments of one call, conflicted, and a conflict
turned the whole file into text with markers. `strata diff` compared printed
lines too, so it could say which lines changed but not which node.

This PR compares and merges definitions node by node (REBUILD.md L5, item 10).
Text only shows the result. On the last 5 compiler commits the node diff of all
changed definitions took 0 to 5 ms, next to 3 to 6 s for the whole command.

# Background
Each side of a merge is read from its module file, written out and parsed back,
then paired with the base by name. Only a definition changed on both sides needs
a real merge. That was `line_merge`:

```strata merge.line_merge
```

`locate` already names nodes by path (`body/2/value/right`) and prints a
definition with nodes marked. Both the new diff view and the paths reuse it:

```strata locate.shown
```

# Intuition
A function has three statements. Ours changes `a := x + 1` into `x + 10`, theirs
changes `b := a * 2` into `a * 20`. The lines are next to each other, so git
conflicts. As trees, the block's list of statements lines up three ways. The
first statement changed on our side only, so it comes from ours. The second
changed on their side only, so it comes from theirs.

If both sides change the `1` in `x + 1` differently, only that literal is in
conflict. It becomes a hole, `x + ?conflict_1(2, 1, 3)`: ours, base, theirs.
The file stays a module file, and `strata holes` lists the hole.

# One walk over every form
`tree.Node` wraps each part of a definition that has children or sits in a
list. `slots` gives a node's children as named slots, and `rebuilt` puts them
back. These two are the only code that knows the tree's forms. A node's head is
the node with its children taken out, so a field another branch adds to the
tree is head data with no change here.

```strata-diff tree.Node tree.slots tree.rebuilt
```

```strata-also
tree.Slot tree.family tree.one tree.many tree.opt_e tree.opt_t tree.es tree.ts tree.ps: building slots
tree.fn_slots tree.decl_slots tree.expr_slots tree.stmt_slots tree.pattern_slots tree.type_slots tree.block_items: slots of each form
tree.at tree.opt_expr tree.expr1 tree.opt_type tree.type1 tree.pat1 tree.exprs tree.types tree.pats tree.fields tree.variants tree.params tree.laws tree.arms tree.comp_fors tree.select_arms tree.named_exprs tree.named_pats tree.entries tree.parts tree.lam_params tree.bounds tree.model_decls tree.model_defs tree.trait_methods tree.fns tree.block_of: taking children back out of slots
tree.fn_rebuilt tree.decl_rebuilt tree.expr_rebuilt tree.stmt_rebuilt tree.pattern_rebuilt tree.type_rebuilt: rebuilding each form
tree.head tree.same_form tree.joined tree.item_path tree.empty_decl tree.use*: heads, forms and paths
```

# Diff node by node
`tree.changes` skips equal subtrees with one `==`. It reports a node of
another form as `replaced`, and a changed head as `edited`, then compares the
children. A list is lined up by its equal items (`matched`). The items left in
a gap are paired by likeness (same form and head), and an item that went to
another place of its list is `moved`.

```strata-diff tree.changes tree.matched tree.gap_pairs
```

```strata-also
tree.Change tree.walk_diff tree.diff_items tree.alike tree.lcs_by: the walk and the alignment
tree.node_text tree.node_at tree.marked_paths: a change's text, and what to mark for it
```

# Merge node by node, conflicts as holes
`tree.merge` takes the side that changed a node. A node both sides changed is
merged by its children when it kept its form. A list is merged like diff3 does
it, after one rule: one side only reordered it, the other changed items in place.
A conflict inside an expression becomes `?conflict_N(ours, base, theirs)`. One
outside every expression (a signature) makes `decl` `None`, and `merge.merge`
falls back to the line merge for that definition.

```strata-diff tree.merge tree.merge_items
```

```strata-diff merge.merge merge_file_command
```

```strata-also
tree.Conflict tree.DeclMerge tree.St tree.merge_node tree.merge_run tree.reordering tree.in_place tree.holds_hole tree.hole tree.side: the merge and its holes
merge.Merged merge.use*: the holes reported
```

# Shown: `diff --nodes` and `--json`
Each changed item of a diff now carries its node changes and both written sides.
`strata diff --nodes` prints each side with the changed nodes marked, then each
change by path. `--json` gives them as `nodes`.

```strata-diff diff.nodes_text diff.node_changes
```

```strata-also
diff.Item diff.NodeChange diff.diff diff.node_text_at diff.marked diff.indented diff.use*: carrying the node changes
diff_command diff_json: the option and the JSON field
```

# Tests
The tests merge and diff small definitions parsed from text: neighbouring
statements, two arguments of one call, statements added at different places,
reordered arms with an edited arm, one node changed twice (a hole), one place
given two different statements (one hole), and a signature (no hole).

```strata-also
tree.test* tree.decl_of tree.merged_text: the cases
```
