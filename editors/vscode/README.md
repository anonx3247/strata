# Strata for VS Code

Syntax highlighting for Strata's text form (`.st`: what `strata view` prints,
`strata edit` opens, and `view --split` writes), with a TextMate grammar
(VS Code doesn't read tree-sitter grammars; `../tree-sitter-strata` is the one
Helix, Neovim and Zed use).

Install it by linking this folder into VS Code's extensions, then reload:

```sh
ln -s "$PWD" ~/.vscode/extensions/strata-lang.strata-0.1.0
```

(`~/.cursor/extensions` for Cursor.) To have VS Code open `strata edit`'s
files, set `EDITOR="code --wait"`.
