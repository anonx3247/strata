# docs-site: the docs, as a website

[Starlight](https://starlight.astro.build) (Astro), served at `strata-lang.dev/docs`. The pages are the repository's own `docs/*.md`: `sync.mjs` copies them into `src/content/docs/` (generated, not committed) with their `# Title` as frontmatter, Strata code fences marked `strata` (they say `rust` so GitHub colours them), links between docs pointing at their pages, and links to other files going to GitHub. Edit `docs/`, not the copies.

```sh
cd docs-site
npm ci
npm run dev       # http://localhost:4321/docs/, reloads as docs/ changes (re-run to pick up a new file)
npm run build     # into ../site/docs/, next to the landing page and the tour
```

- A new page: add `docs/NAME.md`, then list `NAME` in `PAGES` (sync.mjs) and in the sidebar (astro.config.mjs).
- The standard library's pages are `docs/std/`, written by `strata --file std doc -o docs/std` from std's `///` docs (run it again after changing them); sync.mjs makes them the pages under `std/`, their `README.md` the index, and the sidebar lists them by itself.
- `pages/` holds the pages written for the site only (the overview, `index.mdx`).
- Strata highlighting is `src/strata.tmLanguage.json` (a TextMate grammar, usable by editors too) with the colours of `src/strata-dark.json`, which are the landing page's.
- On Vercel, `site/vercel.json` runs this build, so `site/docs/` is never committed.
