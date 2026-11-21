# site: Strata, presented

A presentation of Strata as slides. It starts from how an agent works in a codebase of text files today (an illustration), then shows Strata's three ideas: types as contracts, proving over testing, and the code is the AST. Each comes with recorded examples. It ends with the rest of the language, briefly.

```sh
cargo build --release
node site/build.mjs                       # re-record everything from the real CLI
cd site && python3 -m http.server 8000    # then open http://127.0.0.1:8000
```

Opening `site/index.html` directly from disk works too.

- **Moving around**: ← → (or space, PageUp / PageDown) step through a slide, then on to the next; the dots at the bottom jump to a slide. The address names the slide and the step (`index.html#session/5`), so a link opens there.
- **What is shown**: code typed out as it is written, beside a graph of the codebase (each definition a node, placed by how deep its dependencies go; an edge is a use) that shows what changed, what is re-checked, holes, verdicts and counterexamples. No embedded viewer.
- `build.mjs` runs `strata` on scratch packages (from `examples/`, and small programs it stores with `strata edit`, so one that stops checking fails the build), records each command's output and the codebase after each step, and writes `data/*.js`. Nothing on the slides is written by hand.
- `index.html`, `slides.css` and `app.js` are the deck; `style.css` and `common.js` (Strata highlighting, terminal colouring, steppers) are shared with `docs.html`, which renders `docs/learn-strata.md`, `verification.md` and `tools.md` (recorded into `data/docs.js`). In Tools, the viewer's screenshot is replaced by a tour of the real viewer (`data/tour.js`, in `viewer.html`).
- `viewer.html` is generated from `src/ui/index.html`: the same viewer, forced to its dark theme, answering its API calls from the snapshots the docs post to it. Re-run the build after changing the viewer.
