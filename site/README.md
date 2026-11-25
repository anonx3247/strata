# site: the website

`index.html` is the landing page. It shows the install command (`INSTALL` at the top of `landing.js`), a link to the tour, and functions from `examples/math.strata` typed out with what `strata verify` reports for them (from `data/math.js`). `landing.css` and `landing.js` belong to it.

`tour.html` presents Strata as slides. It opens with an illustration of how an agent works in a codebase of text files, then covers Strata's three ideas, each with recorded examples: types as contracts, proving over testing, and the code is the AST. It ends with a short overview of the rest of the language.

```sh
tools/bootstrap.sh                        # target/strata (or STRATA=… a strata installed)
node site/build.mjs                       # re-record everything from the real CLI
cd site && python3 -m http.server 8000    # then open http://127.0.0.1:8000
```

Opening `site/index.html` or `site/tour.html` directly from disk works too.

- **Navigation:** ← and → (or space, PageUp and PageDown) step through a slide and on to the next. The dots at the bottom jump to a slide. The URL names the slide and step (`tour.html#session/5`), so a link opens there.
- **Content:** code typed out as it is written, next to a graph of the codebase. Each definition is a node, placed by the depth of its dependencies, and each edge is a use. The graph shows what changed, what is re-checked, holes, verdicts and counterexamples.
- `build.mjs` runs `strata` on scratch packages, built from `examples/` and from small programs it stores with `strata edit`, so a program that stops checking fails the build. It records each command's output and the codebase after each step, and writes `data/*.js`. Nothing on the slides is written by hand.
- `tour.html`, `slides.css` and `app.js` make up the deck. `style.css` and `common.js` (Strata highlighting, terminal colouring, steppers) are shared with the landing page.
- The docs (`/docs/`) are a Starlight site built from `docs/*.md` by [docs-site](../docs-site/README.md) into `site/docs/`, which is not committed.

## Hosting

The landing page and the tour are static. Everything they show is committed in `data/`, so hosting them needs no `strata`. On Vercel, import the repository and set **Root Directory** to `site`, keeping "Include files outside the root directory" on. `site/vercel.json` builds the docs (`docs-site`, into `site/docs/`) and serves the folder. To update the tour, re-run `node site/build.mjs` and commit `site/data/`. Changes to `docs/` appear on the site at the next deploy.
