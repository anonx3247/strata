# site: Strata, presented

An interactive page that presents Strata through what it fixes: structural changes instead of line diffs, verification instead of tests that miss cases, contracts that re-check their users, edits that can't break the codebase. It replays a recorded agent session in the real viewer.

```sh
cargo build --release
node site/build.mjs                       # re-record everything from the real CLI
cd site && python3 -m http.server 8000    # then open http://127.0.0.1:8000
```

Opening `site/index.html` directly from disk works too.

- `build.mjs` runs `strata` on scratch packages (from `examples/`), records each command's output and the viewer's `/api/state` and `/api/changes` after each step, and writes `data/*.js`. Nothing shown on the page is written by hand.
- `viewer.html` is generated from `src/ui/index.html`: the same viewer, forced to its dark theme, answering its API calls from the snapshots the page posts to it. Re-run the build after changing the viewer.
- `index.html`, `style.css` and `app.js` are the page.
