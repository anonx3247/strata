# tasks, in Strata

A port of the [tasks](../../../tasks) reference (Python and TypeScript): backend-agnostic task queues, where producers enqueue tasks and workers claim batches of attempts and report results.

```sh
strata --file examples/tasks/tasks.strata test           # the reference test suite
strata --file examples/tasks/tasks.strata run demo       # producer + worker entrypoints
strata --file examples/tasks/tasks.strata verify         # contracts + model-based simulation
strata ui --file examples/tasks/tasks.strata             # browse it
```

## How the reference maps onto Strata

| Reference (Python / TS) | Strata |
|---|---|
| `Task[T: BaseModel]` (Pydantic) | `type Task[P] = { id: TaskId, kind: Kind, payload: P }`. Every type is JSON-serialisable, so no bound is needed. |
| `TaskResult(result: Literal[...], error: BaseModel \| None)` | `Succeeded \| Failed { error: E } \| Skipped`: "success with an error" cannot be represented |
| `TaskPolicy` with `Field(ge=1)` validators | refined field types (`Positive`, `Retries`): an invalid policy can't be constructed |
| `Producer` / `Worker` protocols + `resources` | `effect Producer[P]` / `effect Worker[P, E]`: a handler closes over its own resources |
| `TaskQueue` ABC + `SqliteTaskQueue` | `effect Queue[P, E]` (enqueue / claim / record / release, **with contracts**) + one handler per backend |
| "entrypoints contain only wiring" | an entrypoint *is* only handlers. See `demo`. |
| worker raises → claim released, exception re-raised | a worker can't raise. A misreport comes back as `Err(ResultsMismatch {..})` and the claim is released. |
| SQLite tables and statuses | `MemQueue[P, E]`: a pure state machine whose invariants are part of its type |
| decoder dispatching on `kind` (TS) | not needed: JSON is parsed against the declared types |

## What verification found

`MemQueue` states its invariants in its type, including *a pending task always has an attempt left*. `strata verify` runs `simulate` on random policies and random sequences of producer runs and worker batches (including workers that drop results), through the real `run_producer` / `run_worker` and the in-memory backend. It found a counterexample in about 3 steps:

```
retries = 0
Produce [t2, t0, t5]  →  Work (worker drops one result)  →  release
⇒ t2, t0, t5 are Pending with attempts == max_attempts
```

The reference has the same bug in both Python and TypeScript. `_release` puts tasks back to `pending` even after their last attempt, and `_claim` only selects `attempts < max_attempts`. Such tasks stay `pending` forever: never retried, never marked `failed`. It reproduces against the real Python `SqliteTaskQueue` (`TaskPolicy(retries=0)` plus a worker that returns `[]`). The port fixes it in `mem_release` and has a regression test.

## Gaps compared with the reference

- **No SQLite / SQS backend yet.** These need host bindings (calling out to existing libraries), which the language doesn't have. They would be further handlers of the same `Queue` effect, held to the same interface contracts.
- **No async yet.** `discover` returns a list rather than an async iterator; generators and async will come from fully resumable effect handlers.
- **Different JSON for results.** The wire format differs for `TaskResult` (`{"Failed": {...}}` instead of `"result": "failure"`); matching the reference byte for byte would need serialisation annotations.
- **`max_workers` is unused**, since there is no concurrency.
