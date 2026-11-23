# site: Strata, presented

`index.html` is the landing page: the install line (the command is `INSTALL` at the top of `landing.js`; there is no installer yet, so it says "coming soon"), the way into the tour, and functions from `examples/math.strata` typed out with what `strata verify` said about them (from `data/math.js`). `landing.css` and `landing.js` are its own.

`tour.html` is a presentation of Strata as slides. It starts from how an agent works in a codebase of text files today (an illustration), then shows Strata's three ideas: types as contracts, proving over testing, and the code is the AST. Each comes with recorded examples. It ends with the rest of the language, briefly.

```sh
cargo build --release
node site/build.mjs                       # re-record everything from the real CLI
cd site && python3 -m http.server 8000    # then open http://127.0.0.1:8000
```

Opening `site/index.html` or `site/tour.html` directly from disk works too.

- **Moving around**: ← → (or space, PageUp / PageDown) step through a slide, then on to the next; the dots at the bottom jump to a slide. The address names the slide and the step (`tour.html#session/5`), so a link opens there.
- **What is shown**: code typed out as it is written, beside a graph of the codebase (each definition a node, placed by how deep its dependencies go; an edge is a use) that shows what changed, what is re-checked, holes, verdicts and counterexamples. No embedded viewer.
- `build.mjs` runs `strata` on scratch packages (from `examples/`, and small programs it stores with `strata edit`, so one that stops checking fails the build), records each command's output and the codebase after each step, and writes `data/*.js`. Nothing on the slides is written by hand.
- `tour.html`, `slides.css` and `app.js` are the deck; `style.css` and `common.js` (Strata highlighting, terminal colouring, steppers) are shared with `docs.html`, which renders `docs/learn-strata.md`, `verification.md` and `tools.md` (recorded into `data/docs.js`).
- The docs' pictures (`docs/images`, screenshots of the viewer) are copied to `images/`, so the site serves them itself. After a change to the viewer, take them again from a running `strata ui` and re-run the build.

## Hosting

The folder is a static site: everything it shows is committed in `data/`, so hosting it needs no build and no `strata`. On Vercel, import the repository and set **Root Directory** to `site`; `site/vercel.json` turns the build off and serves the folder as it is. Re-run `node site/build.mjs` and commit `site/data/` to update what the site shows. Links to files of the repository other than the three doc pages go to GitHub.
