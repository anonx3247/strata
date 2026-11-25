# tree-sitter-strata

A tree-sitter grammar for Strata's text form (`.st`: what `strata view` prints, `strata edit` opens, and `view --split` writes). It follows `compiler/parse.strata`. `src/scanner.c` handles the layout rules: `f(x)` is a call and `f (x)` is not, and an index's `[`, a binary `-`, and a postfix `!` or `?` must be on the same line as what they follow.

- Build and test: `npm install`, then `npx tree-sitter generate && npx tree-sitter test`.
- Queries: `highlights.scm` (Helix scopes, last match wins), `locals.scm`, `indents.scm`, `textobjects.scm`.
- Helix: see `../helix/languages.toml`.

`src/parser.c` is generated and committed, since editors build from `src/`. Every module of std, core and the compiler, the examples, and the `.st` test files parse without errors.
