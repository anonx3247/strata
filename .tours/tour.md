---
title: strata tour
---

# Why
The user reviews agents' pull requests but can't read every line. A tour explains
a PR page by page: what each part is for and how it is solved, with the structural
diff of each definition that matters, and the routine changes folded into counts.
It is item 3 of phase 0 of the rebuild plan: the tool the rebuild is reviewed with.

The PR's agent writes the tour as Markdown at `.tours/BRANCH.md`. `strata tour`
shows it in the terminal; `strata tour --check` makes sure nothing in the PR is left
out of it. This page is part of this PR's own tour.

# Reading the tour file
A tour is a front matter (`title`, `base`, `packages`) and pages, each starting at a
`# ` heading. Three fences are the tour's own: `strata-diff NAME…` embeds diffs,
`strata NAME…` shows code as it is now, `strata-routine` lists `names: reason`
lines. Any other fence stays Markdown. Names may be globs, where `*` stands for any
run of characters; without one a pattern is the name itself, which `matches`
promises in its contract.

```strata-diff tour.Tour tour.Page tour.Block tour.RoutineLine tour.parse_tour tour.front_matter tour.routine_line tour.matches
```

The small readers it is made of:

```strata-diff tour.fence_info tour.page_heading tour.uncommented tour.words_of tour.names_in
```

```strata-diff tour.test*globs* tour.test*pages*
```

# What changed, from git
For each package, `package_changes` asks compiler/diff for the changes from the
base to the working tree (`git_range`, then `range_diff`, called as they are). The
base is always the merge-base with `HEAD` of the tour's `base` (default
`origin/main`, else `main`), so commits merged into main since the branch started
don't show as reverted. The packages are those holding the changed `.strata` files,
unless the tour names them.

```strata-diff tour.package_changes tour.setting tour.Setting tour.package_of tour.folder_of tour.package_decls tour.package_name tour.git
```

Only what a page shows is kept of a diff item: `Change`. The changes are cached
under the strata cache, keyed by the base commit and the hashes of the working
tree's definitions, as tab-separated records; the test reads them back unchanged.

```strata-diff tour.Change tour.Model tour.change_of tour.named tour.changes_text tour.changes_from tour.record tour.line_records tour.escaped tour.unescaped tour.with_last
```

```strata-diff tour.test*escaping* tour.test*kept*
```

# Pages as rows
A page is drawn as rows, each maybe a fold that Enter opens. A diff is drawn as
`strata diff` draws it, unchanged code highlighted; a routine line is a count of
the changes it matches. Printed (piped or `--no-pager`) every list is open and
only unchanged code stays folded.

```strata-diff tour.Row tour.Look tour.block_rows tour.page_rows tour.change_rows tour.change_head tour.code_line tour.routine_matches tour.printed
```

The renderer's last page, "Not in the tour", holds every change no page names, and
the files changed outside Strata code.

```strata-diff tour.left_out tour.last_page tour.all_pages
```

```strata-diff tour.row tour.plural tour.marker tour.mark_of tour.segs_text
```

# --check
`problems` lists every name that isn't a changed definition (for a `strata` block:
not a definition at all), and every change no page names. `--check` prints them
and exits 1.

```strata-diff tour.problems
```

# Browsing at a terminal
The keys go through `step`: a view and a key give the next view. It is proven to
keep the page and the cursor of the page list among the pages. `--keys` replays
keys through it on an 80 by 24 screen, which is how the test case checks them.
Rows are cut to the screen's width (`clipped`), escape codes not counted, so that
a long line doesn't wrap and push the screen down.

```strata-diff tour.View tour.step tour.clamp tour.focused tour.screen tour.clipped tour.replayed tour.browse
```

```strata-diff tour.test*clipping*
```

The terminal is c/screen.c: raw mode on the alternate screen, keys by name, the
size, a frame drawn in one write, and the terminal restored at exit and on a
signal.

```strata-diff tour.screen_interactive tour.screen_columns tour.screen_rows tour.screen_begin tour.screen_end tour.screen_draw tour.screen_key
```

# The command
`tour.command` reads the command line, the tour and the changes, then checks,
replays, browses or prints. `main` only adds the command: `tour_command` reads
core and std, which compiler/diff compares with.

```strata-diff tour.command tour.failed tour_command command_line codebase_commands driver.usage_text
```

# The skill
`strata skill tour` prints how an agent writes a tour, and `strata skill install`
now writes it beside the first skill, as `strata-tour/SKILL.md`.

```strata-diff skill.tour_skill_text skill.skill_command
```

# Routine
```strata-routine
tour.use* use*: imports
```
