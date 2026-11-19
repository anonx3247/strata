# Handoff

State as of commit `1a4f611` (all 14 integration tests pass: `cargo test --release`).
Read `README.md` first; it describes the language and tools as they are now.

## Working conventions

- Commit each step (the user wants every change tracked) and push to `origin main` (github.com/anonx3247/strata, private).
- Keep `cargo test --release` green before every commit; check that the examples still round-trip:
  `strata --file F render > a; strata --file a render | diff - a`.
- Verify UI changes with headless Chrome screenshots:
  `"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new --disable-gpu --hide-scrollbars --window-size=1700,1000 --virtual-time-budget=4000 --screenshot=out.png "http://127.0.0.1:PORT/#level=1&sel=core.run_worker"`.
  Start `strata ui --file examples/tasks --port PORT` in the background, and kill **only that port** afterwards (`kill $(lsof -ti tcp:PORT)`); the user may have their own viewers running.
  Viewer errors show in the page banner.
- macOS: no `timeout` command (use `perl -e 'alarm N; exec @ARGV' cmd`), and BSD `sed`.

## Design decisions the user has made (do not revisit without asking)

- **No effects.** Capabilities (database, queue, console, clock) are traits; code takes an implementation as a parameter or holds it in a field. `mock T` generates an implementation that respects the trait's contracts. Async and iterators will be designed later as their own language features, **not** as effects.
- **Immutable by default, mutable when declared**: `let mut`, field assignment, `mut` parameters and `mut self` with write-back to the caller's variable. Procedural code is normal, and there are no shared references.
- **Refinements are structural**; `type X = Y` with no `where` is an error, so use `distinct type` for a nominal type.
- **Modules meet only through their public (`pub`) contracts.** A folder of `.strata` files loads as modules, and `render --split` writes them back.
- **Rendered code wraps at about 80 columns** by breaking lines, not soft-wrapping.
- **Viewer**: two levels (interface/source), the code view sliced around the selection, a type map as a separate view, and no Flow view.

## Next task: viewer UI changes the user asked for (not started)

All of these are in `src/ui/index.html` (functions `card`, `renderCode`, `renderRight`) and `src/ui.rs` (`snapshot`):

1. **Doc comments**: `///` lines are stored on declarations (`Decl.doc`). Remove them from the rendered code in cards, and show them in the right panel under the name, as the description. The JSON already has `doc`.
2. **Right panel**: remove the source block (`insp-src`). Keep the name, description, the mini graph of neighbours, status (holes, verdict), and history.
3. **Code items**: drop the boxed card header. Items should look like plain code with light separators and a small kind/status marker; the name is already in the code.
4. **Interface/source**: show the toggle only where the two texts differ (functions with bodies, impls, traits with default methods), as an inline "show body ▸" / "hide body ▾" control in the code. Remove the global Interface/Source buttons; keep a "Type map" tab, and maybe add "expand all".
5. **Sections**: the main item, then *uses*, *used by*, implementations and tests, following each other in that order.
6. **Type map** (optional): the user suggested it could become a side panel rather than a full tab; ask before doing this.

## Open topics the user wants to discuss and decide on (do not implement without agreement)

- **Verification strategy.** Today: compile-time checks on literals, runtime contract checks, and fuzzing (`strata verify`, with results cached by content hash). The options discussed are summarised at the end of the last conversation and in `README.md` under the roadmap.
- **Transpilation / interop** with Python (Pydantic, JSON as the boundary), TypeScript and Rust. Later.

## Known gaps

- Trait methods must take `self`; there is no `fn empty() -> Self` dispatch yet, and method names must be unique across traits.
- Exhaustiveness checking ignores refinements: a `match` on `NonEmpty` still needs a `[]` arm.
- No host bindings (FFI), so a real SQLite backend isn't possible yet.
- `strata verify` on the tasks example takes about 35s; the slowest part is generating `MemQueue` values that satisfy its invariants.
- `src/flow.rs` and the `flows` data in `ui.rs` are no longer used by the viewer, now that the Flow view is gone. They can be removed.
- Hover arrows in the viewer have not been checked visually; headless Chrome can't hover.
