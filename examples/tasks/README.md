# tasks, in Strata

A port of the [tasks](../../../tasks) reference library (Python and TypeScript): backend-agnostic task queues, where producers enqueue tasks and workers claim batches of attempts and report results.

It has three modules, one per file, which use each other only through their public contracts:

| Module | What it holds |
|---|---|
| `model` | the types (`Task`, `TaskResult`, `TaskPolicy`, `Status`, `Entry`), the `Producer` / `Worker` traits, the `shared trait Queue` with its model and contracts, and the `run_producer` / `run_worker` / `drain` entrypoints |
| `memory` | the in-memory backend: a `MemQueue` value implementing `Queue` (its model is its table), and the reference test suite's scenarios |
| `print` | the reference's example task family and its `demo` entrypoint |

```sh
strata --file examples/tasks test                 # the reference scenarios
strata --file examples/tasks run print.demo       # producer + worker entrypoints
strata --file examples/tasks verify               # the queue's contracts and laws
strata --file examples/tasks interface model       # what the other modules can use
```

## How the reference maps onto Strata

| Reference (Python / TS) | Strata |
|---|---|
| `Task[T: BaseModel]` (Pydantic) | `type Task[P] = { id: TaskId, kind: NonEmptyString, payload: P }`. Every type is JSON-serialisable, so no bound is needed. |
| `TaskResult(result: Literal[...], error: BaseModel \| None)` | `Succeeded \| Failed { error: E } \| Skipped`: "success with an error" cannot be represented |
| `TaskPolicy` with `Field(ge=1)` validators | refined field types (`Positive`, `Natural`): an invalid policy can't be constructed |
| `Producer` / `Worker` protocols + `resources` | `trait Producer[P]` / `trait Worker[P, E]`: an implementation carries its resources in its fields (`PrintWorker { console }`) |
| `TaskQueue` ABC + `SqliteTaskQueue` | `shared trait Queue[P, E]`: what any backend means is its model (`policy`, `entries: Dict[TaskId, Entry[P]]`); enqueue / claim / record / release / expire each say what they do to it, with `old`; laws (`enqueue_once`, `no_double_claim`, `late_worker_refused`); one implementation per backend, held to all of it |
| SQLite `running` rows and visibility timeouts | a lease: `Running { by: WorkerId, until: Instant }`; `record` and `release` by a worker that doesn't hold the lease are refused (`Err(NotYours)`, nothing changed); `expire(now)` gives back overdue tasks |
| "entrypoints contain only wiring" | entrypoints take the queue and worker as values; see `demo` |
| worker raises → claim released, exception re-raised | a worker can't raise. A misreport comes back as `Err(ResultsMismatch {..})` and the claim is released. |
| SQLite tables and statuses | `MemQueue[P, E]`: a table by id plus the arrival order (its refinement: each entry is under its task's id, and the order holds every id); what every entry obeys (attempts within the policy's budget, a pending task has one left…) is `Entry`'s refinement, proven at every write |
| decoder dispatching on `kind` (TS) | not needed: JSON is parsed against the declared types |

## What verification found

`MemQueue`'s invariants were stated in its type, including *a pending task always has an attempt left*. Model-based simulation through the real entrypoints found a counterexample in about 3 steps:

```text
retries = 0
Produce [t2, t0, t5]  →  Work (worker drops one result)  →  release
⇒ t2, t0, t5 are Pending with attempts == max_attempts
```

The reference has the same bug in both Python and TypeScript. `_release` puts tasks back to `pending` even after their last attempt, and `_claim` only selects `attempts < max_attempts`, so such tasks stay `pending` forever: never retried, never marked `failed`. It reproduces against the real Python `SqliteTaskQueue` with `TaskPolicy(retries=0)` and a worker that returns `[]`. The port fixes it: a released task goes through `after_attempt`, which marks it exhausted on its last attempt.

The property is stated once, as `Entry`'s refinement, so every backend must preserve it wherever it writes an entry; a backend that releases to Pending on the last attempt doesn't type-check. The queue's contracts describe the component as a whole, not the input and output of single calls:

- Each method is specified against the model. `claim` hands out only pending tasks, each now `Running` under `by`'s lease with one more attempt, and changes nothing else. `record` and `release` succeed exactly when `by` holds every task and no result is reported twice, and a refused call changes nothing. `expire` gives back exactly the overdue tasks.
- Every method (the trait's `ensures`) keeps the policy unchanged, never removes or alters a task, and starts each new entry as Pending with no attempt.
- Three laws, all proven for `MemQueue`: enqueuing again adds nothing, two claims never hand out the same task, and a worker whose lease expired can't report.
- The queue is `shared`, since many workers use it, so entrypoints rely only on what each call returns: `run_worker` returns one result per claimed attempt, at most a batch, each id once.

`strata --file examples/tasks verify` proves the helpers, `drain`, `run_producer`, the demo, `empty_queue`, `MemQueue.claim` and all three laws. `enqueue`, `record`, `release` and `expire` are fuzzed without a counterexample, with most of their checks proven. The unproven checks are in their loops, whose invariants relating the table to its value before the loop are not inferred, and in `record`'s refusal path (see "Examples over trait models: what the prover lacks" in `IDEAS.md`).

## Gaps compared with the reference

- **No SQLite or SQS backend.** Each would be another implementation of the `Queue` trait, held to the same contracts (std.sqlite provides the SQLite bindings). Checking implementations against the trait's contract already caught `MemQueue.record` requiring more than `Queue.record` promises.
- **`discover` returns a list**, where the reference returns an async iterator.
- **Different JSON for results.** The wire format of `TaskResult` differs (`{"Failed": {...}}` instead of `"result": "failure"`). Matching the reference byte for byte would need serialization annotations.
- **`max_workers` is unused.** Workers are `run_worker` calls, and since the queue is `shared`, verify treats every call as possibly interleaved with other workers'.
