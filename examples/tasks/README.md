# tasks, in Strata

A port of the [tasks](../../../tasks) reference (Python and TypeScript): backend-agnostic task queues, where producers enqueue tasks and workers claim batches of attempts and report results.

Four modules, one per file, which use each other only through their public contracts:

| Module | What it holds |
|---|---|
| `core` | the model (`Task`, `TaskResult`, `TaskPolicy`), the `Producer` / `Worker` / `Queue` traits, and the `run_producer` / `run_worker` / `drain` entrypoints |
| `memory` | the in-memory backend: a `MemQueue` value implementing `Queue`, with the reference SQLite semantics and invariants |
| `print` | the reference's example task family and its `demo` entrypoint |
| `sim` | model-based verification (`simulate`) and the reference test suite |

```sh
strata --file examples/tasks test                 # the reference test suite
strata --file examples/tasks run print.demo       # producer + worker entrypoints
strata --file examples/tasks verify               # contracts + model-based simulation
strata --file examples/tasks interface core       # what the other modules can use
strata ui --file examples/tasks                   # browse it
```

## How the reference maps onto Strata

| Reference (Python / TS) | Strata |
|---|---|
| `Task[T: BaseModel]` (Pydantic) | `type Task[P] = { id: TaskId, kind: NonEmptyString, payload: P }`. Every type is JSON-serialisable, so no bound is needed. |
| `TaskResult(result: Literal[...], error: BaseModel \| None)` | `Succeeded \| Failed { error: E } \| Skipped`: "success with an error" cannot be represented |
| `TaskPolicy` with `Field(ge=1)` validators | refined field types (`Positive`, `Natural`): an invalid policy can't be constructed |
| `Producer` / `Worker` protocols + `resources` | `trait Producer[P]` / `trait Worker[P, E]`: an implementation carries its resources in its fields (`PrintWorker { console }`) |
| `TaskQueue` ABC + `SqliteTaskQueue` | `trait Queue[P, E]` (enqueue / claim / record / release, **with contracts**) + one implementation per backend; operations return the backend's next state |
| "entrypoints contain only wiring" | entrypoints take the queue and worker as values; see `demo` |
| worker raises → claim released, exception re-raised | a worker can't raise. A misreport comes back as `Err(ResultsMismatch {..})` and the claim is released. |
| SQLite tables and statuses | `MemQueue[P, E]`: a pure state machine whose invariants are part of its type |
| decoder dispatching on `kind` (TS) | not needed: JSON is parsed against the declared types |

## What verification found

`MemQueue` states its invariants in its type, including *a pending task always has an attempt left*. `strata verify` runs `simulate` on random policies and random sequences of producer runs and worker batches (including workers that drop results), through the real `run_producer` / `run_worker` and the in-memory backend. It found a counterexample in about 3 steps:

```text
retries = 0
Produce [t2, t0, t5]  →  Work (worker drops one result)  →  release
⇒ t2, t0, t5 are Pending with attempts == max_attempts
```

The reference has the same bug in both Python and TypeScript. `_release` puts tasks back to `pending` even after their last attempt, and `_claim` only selects `attempts < max_attempts`. Such tasks stay `pending` forever: never retried, never marked `failed`. It reproduces against the real Python `SqliteTaskQueue` (`TaskPolicy(retries=0)` plus a worker that returns `[]`). The port fixes it in `mem_release` and has a regression test.

## Gaps compared with the reference

- **No SQLite / SQS backend yet.** These need host bindings (calling out to existing libraries), which the language doesn't have. They would be further implementations of the `Queue` trait, held to the same contracts. Testing implementations against the trait's contract already caught `MemQueue.record` demanding more than `Queue.record` promises.
- **No async yet.** `discover` returns a list rather than an async iterator; async and iterators will be designed as language features.
- **Different JSON for results.** The wire format differs for `TaskResult` (`{"Failed": {...}}` instead of `"result": "failure"`); matching the reference byte for byte would need serialisation annotations.
- **`max_workers` is unused**, since there is no concurrency.
