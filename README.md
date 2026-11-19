# Strata

An experimental programming language designed for working with agents.

- **Code is a database, not files.** Definitions are stored as syntax trees, addressed by content hash. Text is a *view*, rendered at whatever level of detail you need, and edits are parsed back into the tree.
- **Every change is checked and atomic.** An edit is committed only if the *whole* codebase still type-checks. The log records whether each change touched an interface or only an implementation, and which definitions depend on it.
- **Incomplete programs still check.** Typed holes (`?name`) have a goal type, a context and contract obligations, so a sketch made of signatures and contracts can be verified before it is implemented, and each hole is a precise task to hand to an agent.
- **Constraints live in the types.** Refinement types (`Int where 1 <= it && it <= 99`, `String where matches(it, "...")`), `requires` / `ensures` contracts, exhaustive pattern matching, and algebraic data types.
- **Side effects are declared, so mocks are free.** Effects such as `Db` appear in signatures. Handlers supply implementations, and `with mock Db` generates values that satisfy the declared return types.
- **Everything is JSON.** Every value serializes to JSON automatically. JSON inputs are parsed against the types, with every refinement validated and errors pointing at the exact path.

> Status: **first experimental draft** (~4.3k lines of Rust). Contracts are checked at runtime and by
> property-based fuzzing, not yet *proven*. See [the roadmap](#roadmap).

## Try it

```sh
cargo build --release
alias strata=$PWD/target/release/strata

# Work directly on a file…
strata --file examples/orders.strata check
strata --file examples/orders.strata test
strata --file examples/orders.strata verify

# …or in a codebase database
mkdir /tmp/shop && cd /tmp/shop
strata init
strata add path/to/examples/orders.strata
strata view --level 0              # map of everything
strata view slice:Order --level 1  # Order, what it uses, what uses it: interfaces only
strata holes                       # what's left to implement
```

## A taste

```
type OrderId  = String where matches(it, "ord_[a-z0-9]{6}")
type Quantity = Int where 1 <= it && it <= 99
type Cents    = Int where it >= 0
type NonEmpty[T] = List[T] where len(it) > 0

type Line = { sku: String, qty: Quantity, unit_price: Cents }

type Order =
  | Pending   { id: OrderId, lines: NonEmpty[Line] }
  | Shipped   { id: OrderId, lines: NonEmpty[Line], tracking: String }
  | Cancelled { id: OrderId, reason: String }

effect Db {
  fn load(id: OrderId) -> Option[Order]
  fn save(o: Order) -> ()
}

fn ship(id: OrderId, tracking: String) -> Result[Order, ShipError] / {Db}
  ensures match it { Ok(o) => o is Shipped, Err(_) => true }
{
  match Db.load(id) {
    None => Err(NotFound),
    Some(Pending { id, lines }) => {
      let o = Shipped { id, lines, tracking }
      Db.save(o)
      Ok(o)
    }
    Some(_) => Err(NotPending),
  }
}

fn refund(o: Order) -> Cents
  ensures it >= 0
  = ?refund_policy          // a typed hole: callers still check

test "auto-mocked storage never breaks ship's contract" {
  let r = handle ship("ord_zzz999", "TRK9") with mock Db
  assert r is Ok || r is Err
}
```

## What the tools tell you

```text
$ strata check                               # static errors are caught before running
error in status: non-exhaustive match on `Order`: `Cancelled { .. }` is not covered
error in sneaky: performs `Db.load` but `Db` is neither declared by `sneaky` nor handled here
error in bad_qty: refinement violated at compile time in field `Line.qty`: 120 does not satisfy `Quantity` (where 1 <= it && it <= 99)

$ strata holes
?refund_policy  in refund
  goal:    Cents  (= Int + refinement)
  context: o: Order
  must:    ensures it >= 0

$ strata verify                              # fuzz every contract, with generated inputs and mocked effects
✓ ship                     200 cases, 0 discarded by `requires`
✗ refund                   Refinement violation
    input:   [{"Shipped":{"id":"ord_1t6kdl","lines":[{"sku":"vbehwh","qty":993,"unit_price":0}],"tracking":"27c7p"}}]
    result of `refund`: -500 does not satisfy `Cents` (where it >= 0) (stack: refund)

$ strata edit -m "steeper discount" < discount.strata
fn     discount                 changed  #fc40aad7… → #2e54e58d…  (implementation only — dependents unaffected)

$ strata edit < breaking_change.strata
error in test "totals": call to `total`, argument 1: expected `Int`, found `Line`
rejected: the codebase would not check; nothing was changed

$ strata run ship '["ord_abc123","TRK1"]' --mock --trace
{"event":"call","fn":"ship","args":["ord_abc123","TRK1"],"depth":1}
{"event":"perform","op":"Db.load","args":["ord_abc123"],"result":{"Some":{"Cancelled":{"id":"ord_so7f76","reason":"ju_4p-"}}}}
{"event":"return","fn":"ship","value":{"Err":"NotPending"},"depth":1}
```

## Views (levels of detail)

| Level | Shows |
|---|---|
| `--level 0` | a map: one line per definition, with its effects, dependencies, holes and verification status |
| `--level 1` | interfaces only: types, effects, signatures and contracts |
| `--level 2` | an outline: L1 plus a one-line summary of each body |
| `--level 3` | full source; `render` output parses back to the same tree |

Selectors: `name`, `prefix*`, `slice:Name` (Name, what it uses, and what uses it), or a comma-separated combination.
Every view is Strata text, so an agent can edit what it sees and send it back with `strata edit`.

## How it is stored

```
.strata/
  index.json          name → hash, in order
  objects/<hash>.json the syntax tree of one definition (content-addressed)
  log.jsonl           one entry per commit: which definitions changed, interface vs. implementation, which dependents are affected
  verdicts.json       verification results, keyed by the hash of a definition *and everything it depends on*
```

Because results are keyed by content, `verify` reruns only what is affected by a change.

## Source layout

| File | What it is |
|---|---|
| `src/ast.rs` | the syntax tree (the canonical representation) |
| `src/lexer.rs`, `src/parser.rs` | text → tree |
| `src/printer.rs` | tree → text (canonical form, used for hashing) |
| `src/types.rs` | type inference, effect checking, exhaustiveness, holes |
| `src/interp.rs` | interpreter: effect handlers, runtime contracts, JSON in/out, traces |
| `src/generate.rs` | value generators derived from types and refinements (used by mocks and fuzzing) |
| `src/verify.rs` | compile-time refinement checks on literals; contract fuzzing |
| `src/store.rs` | content-addressed codebase, dependency graph, patch log |
| `src/view.rs` | level-of-detail projections and selectors |

## Known limitations of this draft

- **Verification is fuzzing plus runtime checks, not proof.** Next step: an SMT backend (Z3 or cvc5) that proves what it can and tags the rest as `fuzzed` or `runtime-checked`.
- **No traits or protocols yet.** Generics exist, but there is no way to constrain them.
- **Limited effect handlers.** A handler returns one value straight back to the operation; it cannot resume the computation later or run it more than once (no generators or async yet). Handlers also cannot keep state between calls.
- **Lambdas are not effect-polymorphic.** A closure's effects are checked where it is defined, not where it is called.
- Refinements on generic type arguments, such as `Option[Quantity]`, are not checked when values are constructed.
- **No modules, no agent server or UI, no compilation backend.** It is a tree-walking interpreter.

## Roadmap

1. **SMT-backed refinements**: obligations proven automatically where possible; counterexamples from the solver.
2. **Traits with laws**: e.g. `trait Monoid { … law assoc(a, b, c): … }`; laws are verified or fuzzed for every implementation.
3. **An agent API (MCP server)**: `view`, `edit`, `check`, `holes`, `verify`, `run` and trace queries as tools; scoped workspaces and leases for multiple agents.
4. **Better effects**: full continuations, stateful handlers, `mock record` / `mock replay` for integration tests, and deterministic replay debugging.
5. **A map UI**: a zoomable L0→L3 view showing live agent activity, open holes, and verification status.
6. **Structural records with row polymorphism**: "anything with `.id: OrderId`".
7. **A compilation backend** (WasmGC or Cranelift).
