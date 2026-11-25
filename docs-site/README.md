# docs-site: the docs as a website

A [Starlight](https://starlight.astro.build) (Astro) site, served at `strata-lang.dev/docs`. Its pages are the repository's own `docs/*.md`. `sync.mjs` copies them into `src/content/docs/`, which is generated and not committed. While copying, it:

- turns each `# Title` into frontmatter
- marks Strata code fences as `strata` (the sources say `rust` so GitHub highlights them)
- points links between docs at their pages, and links to other files at GitHub

Edit `docs/`, not the copies.

```sh
cd docs-site
npm ci
npm run dev       # http://localhost:4321/docs/, reloads as docs/ changes (re-run to pick up a new file)
npm run build     # into ../site/docs/, next to the landing page and the tour
```

- To add a page, create `docs/NAME.md`, then list `NAME` in `PAGES` (sync.mjs) and in the sidebar (astro.config.mjs).
- The standard library's pages are `docs/std/`, generated from std's `///` docs by `strata --file std doc -o docs/std`. Regenerate them after changing those docs. sync.mjs publishes them under `std/`, with their `README.md` as the index, and the sidebar lists them automatically.
- `pages/` holds pages written only for the site (the overview, `index.mdx`).
- Strata highlighting is `src/strata.tmLanguage.json`, a TextMate grammar that editors can use too, with the landing page's colours from `src/strata-dark.json`.
- On Vercel, `site/vercel.json` runs this build, so `site/docs/` is never committed.
