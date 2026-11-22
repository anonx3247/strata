# Working on Strata

Keep track of decisions, open questions and ideas in [IDEAS.md](IDEAS.md); the README and `docs/` describe what is built.

## Conventions

- Commit each step (the user wants every change tracked) and push to `origin main` (github.com/anonx3247/strata, private).
- Never run the whole test suite (CI doesn't run `cargo test` either). Run only the tests for what you are
  changing, by name: `cargo test --release --test cli <filter>` (or `--lib <filter>` for unit tests), with
  filters matching the feature (`verify`, `json`, `regex`, …) plus any test you add; they must pass before
  every commit. Check that the examples still round-trip:
  `strata --file F render > a.st; strata --file b.strata edit < a.st; strata --file b.strata render | diff - a.st`.
- `.strata` files are binary module files (docs/binary-format.md): write code with `strata edit` (or
  `strata --file F edit < text` outside a package), read it with `strata render`. A change to the tree
  re-encodes the repository's files ("Changing the format" in that doc); the format version stays 1 until
  the format is settled (user's call), so don't bump it.
- Verify UI changes with headless Chrome screenshots:
  `"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new --disable-gpu --hide-scrollbars --window-size=1700,1000 --virtual-time-budget=4000 --screenshot=out.png "http://127.0.0.1:PORT/#level=1&sel=core.run_worker"`.
  Start `strata ui --file examples/tasks --port PORT` in the background, and kill **only that port** afterwards (`kill $(lsof -ti tcp:PORT)`); the user may have their own viewers running.
  Viewer errors show in the page banner.
  To check hover: start Chrome with `--headless=new --remote-debugging-port=9333 --user-data-dir=<scratch>`, then a small Node 22 script (built-in `WebSocket`, Chrome DevTools protocol) opens the page, sends `Input.dispatchMouseEvent` (`mouseMoved`) over an `a.ref`, and calls `Page.captureScreenshot`. Kill only that port afterwards.
- macOS: no `timeout` command (use `perl -e 'alarm N; exec @ARGV' cmd`), and BSD `sed`.
