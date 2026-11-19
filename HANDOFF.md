# Handoff

State as of the viewer UI commit (all 14 integration tests pass: `cargo test --release`).
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

## Viewer UI changes (done)

Doc comments are shown as the description in the right panel (and stripped from the code, `strip_doc` in `ui.rs`); the inspector has no source block; code items are plain code with a margin marker; bodies open inline (`has_body` in the snapshot; impls and traits now have a signatures-only level 1 in `view.rs`); the header has *Code* / *Type map* tabs and the code toolbar has *expand all*. The selected definition starts open.

Still open: **type map as a side panel** instead of a full tab. The user suggested it; ask before doing it.

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
