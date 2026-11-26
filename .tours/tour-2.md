---
title: strata tour, folded and drawn
---

# Why
Tours of real PRs showed three problems. A PR that adds methods gave pages of
`+` lines, because every added body was shown whole. A change was hard to picture
without hand-drawn diagrams. And the tour skill went straight from the objective
to the code, saying nothing of the system the change sits in or of its idea.

Reading them raised more: a page embedded every helper's diff beside the key
changes, Enter could only open the first fold on the screen, a body wrapped in a
new `for` showed every line inside as changed, and added and removed lines lost
their syntax colors.

So pages start folded to their shape, a fold cursor opens any fold, a page diffs
only the definitions that carry its point (`strata-also` names the rest), a tour
can show graphs Strata computes, the skill asks for Background and Intuition
pages, compiler/diff ignores indentation, and diff lines keep their colors.

# Background: how a tour is drawn
A tour file is parsed into pages of blocks. compiler/diff gives each changed
definition as a `Change`: a header (mark, name, `what` words) and its lines, in
which unchanged runs are already `fold` lines. A page is drawn as rows; a row may
open and close a fold.

```strata tour.Row
```

Keys go through `step`, a view and a key to the next view, proven to keep the
page and the list's cursor among the pages. Before this PR, Enter toggled the
first fold on the screen (`focused`).

```strata tour.focused
```

# Intuition
A page should show its shape first. Say a PR adds `cancel` (6 lines) and changes
one line of `ship` (40 lines). The page shows `cancel`'s signature and a fold
`body: 5 lines`, then `ship`'s changed line with a line of context, its 37
unchanged lines in folds. Tab moves a cursor from fold to fold and Enter opens the
one selected.

The diff of a page names few definitions: here, `change_rows` is the point of the
folding, and what it calls are its helpers, listed in a `strata-also` block.

```strata-refs tour.change_rows
```

Where the code moves the values of a sum type, a `strata-graph` block draws the
moves. No sum type of this PR has such moves; the case
`cli-tour_shows_graphs_refs_and_bodies` shows one: adding `cancel` adds the move
`Pending -> Cancelled` to `Order`.

# Folded by default
An added or removed definition shows its doc's first sentence and its interface,
its body behind a fold. A changed one keeps its changed lines and their context,
and folds unchanged runs, its doc's change and the rest of any changed run longer
than 12 lines; past 20 changed lines, the rest of the definition is one fold.
`full` on the fence line opens them.

```strata-diff tour.change_rows tour.line_rows tour.interface_of
```

```strata-also
tour.Change tour.code_lines tour.package_changes tour.changes_text tour.changes_from tour.test*kept*: the interface, looked up when the diff is taken, kept in the cache
tour.code_rows tour.fold_row tour.marker tour.is_changed tour.hunk_limit tour.hunk_shown tour.changed_budget tour.short_body: drawing the folds
tour.Look tour.page_rows tour.printed tour.browse tour.replayed tour.last_page: fold ids per page; printed, every fold closed
```

# A fold cursor
`View` gains the fold selected. `selected` is it while it is on the screen, else
the first fold shown, so scrolling moves the selection with the screen. Tab and
`]` select the next fold, Shift-Tab and `[` the one before, scrolling it into
view; Enter opens or closes it. `step` is still proven.

```strata-diff tour.View tour.step tour.selected
```

```strata-also
tour.next_fold tour.scrolled tour.screen tour.test*cursor*: finding, scrolling to and drawing the selected fold
```

# Supporting changes, named and not diffed
`strata-also` takes lines like `strata-routine`'s: names, a colon, what they do for
the page. Each line shows folded as a count; opened, each change's diff opens from
its own line. `--check` counts its names as covered.

```strata-diff tour.Block tour.problems
```

```strata-also
tour.parse_tour tour.Names tour.names_in tour.left_out tour.graph_types tour.test*pages*: reading the new blocks and their names
tour.block_rows: drawing each block
```

# Graphs and refs
A graph needs the solver, which compiler/main has. So the command is split into
`prepare` (read the tour, find the changes) and `show`. Between them, main's
`tour_graph` computes each graph (never for `--check`), cached by the package's
state and the type. `strata-refs` reads `store.graph` once, when a refs block is
there.

```strata-diff tour_graph tour.graph_rows tour.refs_rows
```

```strata-also
tour.Side tour.side_range tour.Prepared tour.prepare tour.show tour.command tour.Model tour.GraphShown tour_command: prepare, main adds the graphs, show
tour.graph_shown_text tour.graph_shown_from tour.test*graphs* tour.wrapped_items: caching graphs, wrapping refs
```

# Indentation is not a change
compiler/diff pairs lines by a longest common subsequence. It now compares them
without their leading whitespace, so a line indented anew is unchanged and folds
with its neighbours. A piece of a changed line that is only spaces is never a
change.

```strata-diff diff.line_diff diff.fine_segments
```

```strata-also
diff.segments diff.test*spaces*: a coarse replacement never adds spaces alone
```

# Diff colors
`diff_line` paints the lines of `strata diff` and of the tour. An added or removed
line keeps its syntax colors on a dark band, the mark bold green or red; plain code
on the band is light gray, so it reads on dark and light terminals alike. Without
color nothing changes.

```strata-diff highlight.tinted highlight.diff_line
```

```strata-also
highlight.test*colored*: the colored lines still read as the text
```

# The skill
The tour skill asks for Why, then Background, Intuition and the Code pages. A Code
page diffs the 3 or 4 definitions that carry its point and lists the rest in
`strata-also`.

```strata-diff skill.tour_skill_text
```

# Routine
```strata-routine
tour.use*: imports
```
