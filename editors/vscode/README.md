# Strata for VS Code

Syntax highlighting for Strata's text form (`.st`: what `strata view` prints, `strata edit` opens, and `view --split` writes), using a TextMate grammar. VS Code doesn't read tree-sitter grammars; `../tree-sitter-strata` is the grammar for Helix, Neovim and Zed.

To install, link this folder into VS Code's extensions folder, then reload:

```sh
ln -s "$PWD" ~/.vscode/extensions/strata-lang.strata-0.1.0
```

For Cursor, use `~/.cursor/extensions`. To have VS Code open `strata edit`'s files, set `EDITOR="code --wait"`.
