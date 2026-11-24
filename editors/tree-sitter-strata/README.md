# tree-sitter-strata

A tree-sitter grammar for Strata's text form (`.st`: what `strata view`
prints, `strata edit` opens, and `view --split` writes). It follows
`compiler/parse.strata`; `src/scanner.c` handles the layout rules (`f(x)` is
a call and `f (x)` is not; an index's `[`, a binary `-`, a postfix `!`/`?`
are on the line of what they follow).

- `npm install` then `npx tree-sitter generate && npx tree-sitter test`
- queries: `highlights.scm` (Helix scopes, last match wins), `locals.scm`,
  `indents.scm`, `textobjects.scm`
- Helix: see `../helix/languages.toml`

`src/parser.c` is generated and committed, since editors build from `src/`.
Every module of std, core and compiler, the examples and the `.st` test
files parse with no errors.
