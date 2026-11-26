---
title: strata tour, folded and drawn
---

# Why
Tours of real PRs showed three problems. A PR that adds methods gave pages of
`+` lines: every added body shown whole. A change was hard to picture without
hand-drawn diagrams. And the tour skill went straight from the objective to the
code, with nothing on the system the change sits in or on its idea.

Two smaller ones came with them: a body wrapped in a new `for` showed every line
inside as changed (only its indentation moved), and added and removed lines lost
their syntax colors.

So: everything that can dominate a page starts folded, a tour can show graphs
Strata computes, the skill asks for Background and Intuition pages, compiler/diff
ignores indentation, and diff lines keep their colors on a tinted band.

# Background: how a tour is drawn
A tour file is parsed into pages of blocks. compiler/diff gives each changed
definition as a `Change`: a header (mark, name, `what` words) and its lines, in
which unchanged runs are already `fold` lines. A page is drawn as rows; a row may
open and close a fold, and Enter toggles the first fold on the screen.

```strata tour.Row tour.focused
```

Browsing goes through `step`, proven to keep the page and the list's cursor among
the pages. It is unchanged here.

```strata tour.step
```

# Intuition
A page should show its shape first. Say a PR adds `cancel` (6 lines) and changes
one line of `ship` (40 lines). Before, the page held 6 + 40 lines. Now it holds
`cancel`'s signature and a fold `body: 5 lines`, then `ship`'s changed line with a
line of context around it, its 37 unchanged lines in folds. The reader opens what
they want.

A fold's id now holds its page's number (`2-0:body`), so the set of open folds
remembers each page's folds while the reader moves between pages.

Where the code moves the values of a sum type, `strata-graph` draws the moves.
This PR's own `Block` gains variants but no function moves values between them,
so the case `cli-tour_shows_graphs_refs_and_bodies` shows one instead: adding
`cancel` adds the move `Pending -> Cancelled`.

`strata-refs` shows where a definition sits, one level each way:

```strata-refs tour.change_rows
```

# Folded by default
An added or removed definition keeps its interface, as `view --level 1` prints it,
with its doc's first sentence: compiler/diff's items have no declaration, so
`package_changes` looks it up in the head (added) or the base (removed), as
written, and the cache keeps it (`F` records, `tour-changes-2`).

```strata-diff tour.Change tour.interface_of tour.code_lines tour.package_changes tour.changes_text tour.changes_from tour.test*kept*
```

The rows: `change_rows` folds the doc change and an added or removed body;
`line_rows` folds unchanged runs and cuts a changed run of more than 12 lines to
its first 6. `full` on the fence line opens them all. A `strata` block folds its body the
same way (`code_rows`). A body of at most 3 lines is never folded.

```strata-diff tour.change_rows tour.line_rows tour.code_rows tour.fold_row tour.marker tour.is_changed tour.hunk_limit tour.hunk_shown tour.short_body
```

Printed output is what a reader first sees at a terminal: every fold closed. Fold
ids hold the page's number (`page_rows`); `Look` lost `all_open`.

```strata-diff tour.Look tour.block_rows tour.page_rows tour.printed tour.screen tour.browse tour.replayed tour.last_page
```

# Graphs and refs
Two blocks, parsed with the others; `full` is read on the fence line only.

```strata-diff tour.Block tour.parse_tour tour.Names tour.names_in tour.graph_types tour.left_out tour.test*pages*
```

A graph needs the solver, which compiler/main has: the command is split into
`prepare` (read the tour, find the changes) and `show`, and main's `tour_command`
computes each graph in between (`tour_graph`, not for `--check`), cached by the
package's state (`Side.state`) and the type. `command` is gone: main calls the two.

```strata-diff tour.Side tour.side_range tour.Prepared tour.prepare tour.show tour.command tour.Model tour.GraphShown tour_command tour_graph
```

```strata-diff tour.graph_rows tour.graph_shown_text tour.graph_shown_from tour.test*graphs*
```

`strata-refs` reads `store.graph` over the tour's packages, computed once when a
refs block is there.

```strata-diff tour.refs_rows tour.wrapped_items
```

`--check` knows the new blocks: a refs name must be a definition, a graph type a
sum type.

```strata-diff tour.problems
```

# Indentation is not a change
compiler/diff pairs lines by a longest common subsequence. It now compares them
without their leading whitespace, so a line only indented anew is kept as
unchanged and folds with its neighbours. Inside a changed line, a run of spaces is
never a changed word, and a coarse replacement never adds spaces alone.

```strata-diff diff.line_diff diff.fine_segments diff.segments diff.test*spaces*
```

# Diff colors
`diff_line` paints the lines of `strata diff` and of the tour. An added or removed
line keeps its syntax colors on a dark band, the mark bold green or red; plain code
on the band is light gray, so it reads on dark and light terminals alike. Without
color nothing changes.

```strata-diff highlight.diff_line highlight.tinted highlight.test*colored*
```

# The skill
The tour skill asks for Why, then Background, Intuition and the Code pages, says
how pages read folded, and documents `full`, `strata-graph` and `strata-refs`.

```strata-diff skill.tour_skill_text
```

# Routine
```strata-routine
tour.use*: imports
```
