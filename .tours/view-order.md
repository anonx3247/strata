---
title: view in reading order
packages: compiler
---

# Why
`strata view MODULE`, a selection, `view --split` and the text `strata edit` opens printed definitions in stored order, which tells a reader nothing. They now print them the way a file is read: imports, then types, then functions, then tests; in types and in functions, the roots first (`main`, then what nothing else in the module uses), each followed depth first by what it uses, in the order it names them. A parser's `parse` comes above the parse functions it calls, a union above its members. Only the rendering changes: the stored order stays, and `edit` maps text back by name.

# The order
`order` splits each module into the four groups, and `depth_first` walks the types and the functions from their roots. A cycle is entered where it is first used; what no root reaches follows in stored order.

```strata-diff views.order views.reading_order views.reading_group views.depth_first
```

# What a definition uses: the tree as shown
The edges are the names each definition refers to, as the view writes it, matched to the module's definitions by name (a union's inline variants count as the union). The headers' dependency lists were the first choice, but they describe the stored tree, which the view can write otherwise (a union by its name), and some are stale: a module read back from its view came out in another order. With the shown tree, the order of a read-back module is the view's.

```strata-diff views.names_in views.type_names views.expr_names views.own_names views.variant_names
```

# Every definition exactly once
`placed` sorts `0..n` by each index's first place in the computed order, so the result is a permutation of the input whatever the walk gives. Its length is proven.

```strata-diff views.placed
```

# Where views use it
A module view, `--split` and the editor's text go through `order`. A selection is shown in the order its modules' views use, so picking `parse,sum,digits` keeps `sum` above `digits`. `imports_first` is gone: imports are `order`'s first group.

```strata-diff views.view view_command edit_in_editor imports_first
```

# Routine
```strata-routine
views.use*: imports for the walk and the declaration kinds
```
