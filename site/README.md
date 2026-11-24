# site: Strata, presented

`index.html` is the landing page: the install line (the command is `INSTALL` at the top of `landing.js`; there is no installer yet, so it says "coming soon"), the way into the tour, and functions from `examples/math.strata` typed out with what `strata verify` said about them (from `data/math.js`). `landing.css` and `landing.js` are its own.

`tour.html` is a presentation of Strata as slides. It starts from how an agent works in a codebase of text files today (an illustration), then shows Strata's three ideas: types as contracts, proving over testing, and the code is the AST. Each comes with recorded examples. It ends with the rest of the language, briefly.

```sh
tools/bootstrap.sh                        # target/strata (or STRATA=… a strata installed)
node site/build.mjs                       # re-record everything from the real CLI
cd site && python3 -m http.server 8000    # then open http://127.0.0.1:8000
```

Opening `site/index.html` or `site/tour.html` directly from disk works too.

- **Moving around**: ← → (or space, PageUp / PageDown) step through a slide, then on to the next; the dots at the bottom jump to a slide. The address names the slide and the step (`tour.html#session/5`), so a link opens there.
- **What is shown**: code typed out as it is written, beside a graph of the codebase (each definition a node, placed by how deep its dependencies go; an edge is a use) that shows what changed, what is re-checked, holes, verdicts and counterexamples. No embedded viewer.
- `build.mjs` runs `strata` on scratch packages (from `examples/`, and small programs it stores with `strata edit`, so one that stops checking fails the build), records each command's output and the codebase after each step, and writes `data/*.js`. Nothing on the slides is written by hand.
- `tour.html`, `slides.css` and `app.js` are the deck; `style.css` and `common.js` (Strata highlighting, terminal colouring, steppers) are shared with the landing page.
- The docs (`/docs/`) are a Starlight site built from `docs/*.md` by [docs-site](../docs-site/README.md) into `site/docs/` (not committed).

## Hosting

The landing page and the tour are static: everything they show is committed in `data/`, so they need no `strata` to host. On Vercel, import the repository and set **Root Directory** to `site` (keep "Include files outside the root directory" on); `site/vercel.json` builds the docs (`docs-site`, into `site/docs/`) and serves the folder. Re-run `node site/build.mjs` and commit `site/data/` to update what the tour shows. A change to `docs/` shows on the site at the next deploy.
