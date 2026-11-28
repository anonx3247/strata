// Goals: the obligations of examples/tasks' memory module that #677 no longer proves
// (per-instance quantifier facts). Definitions as in examples/tasks/model.strata,
// examples/tasks/memory.strata and std/time.strata.

/// Nanoseconds since the clock's own start. Only the difference of two
/// instants means something.
pub distinct type Instant = Int

/// Identifies a task. The reference uses a uuid4 hex string.
pub type TaskId = String
  where matches(it, "[A-Za-z0-9_-]{1,64}")

/// Identifies a worker holding a lease.
pub type WorkerId = String
  where 1 <= len(it) && len(it) <= 32

/// A unit of work. `kind` is the task family, e.g. "print". Every Strata
/// type can be written as JSON, so the payload needs no bound.
pub type Task[P] = { id: TaskId, kind: NonEmptyString, payload: P }

/// A single attempt at running a task. Attempt numbers are 1-based.
pub type TaskAttempt[P] = { task: Task[P], attempt: Positive }

/// The outcome a worker reports for one attempt. An error exists only on a
/// failure, so "success with an error" can't be written.
pub type TaskResult[P, E] =
  | Succeeded { task: Task[P] }
  | Failed { task: Task[P], error: E }
  | Skipped { task: Task[P] }

/// Where a task is. A running task is leased: held by one worker until an
/// instant, after which `expire` gives it back.
pub type Status =
  | Pending
  | Running { by: WorkerId, until: Instant }
  | Done
  | Exhausted

/// One task in a queue: the task, where it is, how many attempts it had
/// and may have (`max`, fixed by the policy when it was enqueued). Every
/// entry, in every state of every queue, obeys:
/// - never more attempts than allowed.
/// - a pending task has one left, else it would be stuck: never claimed,
/// never exhausted.
/// - an exhausted one used them all.
/// - a running one is on one.
pub type Entry[P] = {
  task: Task[P],
  status: Status,
  attempts: Natural,
  max: Positive,
}
  where it.attempts <= it.max
    && (it.status is Pending ==> it.attempts < it.max)
    && (it.status is Exhausted ==> it.attempts == it.max)
    && (it.status is Running ==> it.attempts >= 1)

pub fn task_of[P, E](r: TaskResult[P, E]) -> Task[P] {
  match r {
    Succeeded { task } => task,
    Failed { task, .. } => task,
    Skipped { task } => task,
  }
}

/// The ids the results are for.
pub fn ids[P, E](results: List[TaskResult[P, E]]) -> List[TaskId]
  ensures len(it) == len(results) {
  results.map(r -> task_of(r).id)
}

/// Where a task goes once its attempt is over: a failure back to Pending
/// while attempts are left, else Exhausted; a success or a skip to Done.
pub fn after_attempt[P](e: Entry[P], failed: Bool) -> Entry[P] {
  if !failed {
    Entry { ..e, status: Done }
  } else if e.attempts < e.max {
    Entry { ..e, status: Pending }
  } else {
    Entry { ..e, status: Exhausted }
  }
}

/// `es` with the attempt `a` claimed by `by` until `until`: the task was
/// pending, with one attempt fewer, and is now running.
pub fn locked[P](
  before: Dict[TaskId, Entry[P]],
  after: Dict[TaskId, Entry[P]],
  a: TaskAttempt[P],
  by: WorkerId,
  until: Instant,
) -> Bool {
  match before.get(a.task.id) {
    Some(e) => e.task == a.task
      && e.status == Pending
      && e.attempts + 1 == a.attempt
      && after.get(a.task.id) == Some(Entry {
        ..e,
        status: Running { by, until },
        attempts: a.attempt,
      }),
    None => false,
  }
}

/// Every attempt claimed by `by` until `until` (`locked`).
pub fn all_locked[P](
  before: Dict[TaskId, Entry[P]],
  after: Dict[TaskId, Entry[P]],
  attempts: List[TaskAttempt[P]],
  by: WorkerId,
  until: Instant,
) -> Bool {
  all(attempts, a -> locked(before, after, a, by, until))
}

/// The result `r` recorded: its task's entry moved on from its attempt.
pub fn settled[P, E](
  before: Dict[TaskId, Entry[P]],
  after: Dict[TaskId, Entry[P]],
  r: TaskResult[P, E],
) -> Bool {
  id := task_of(r).id
  match before.get(id) {
    Some(e) => after.get(id) == Some(after_attempt(e, r is Failed)),
    None => false,
  }
}

/// The running task `id` given back: Pending with attempts left, else
/// Exhausted (never stuck pending without one).
pub fn unlocked[P](
  before: Dict[TaskId, Entry[P]],
  after: Dict[TaskId, Entry[P]],
  id: TaskId,
) -> Bool {
  match before.get(id) {
    Some(e) => after.get(id) == Some(after_attempt(e, true)),
    None => false,
  }
}

/// The ids of the first `i` tasks.
fn ids_upto[P](tasks: List[Task[P]], i: Int) -> Set[String] {
  set(tasks[..i].map(t -> t.id))
}

/// Some of the first `i` tasks is new to `d`.
fn some_new[P](
  d: Dict[TaskId, Entry[P]],
  tasks: List[Task[P]],
  i: Int,
) -> Bool {
  any(tasks[..i], t -> !d.contains(t.id))
}

/// `order` holds every key of `table`.
pub fn listed[P](table: Dict[TaskId, Entry[P]], order: List[TaskId]) -> Bool {
  all(table.items(), (id, _) -> order.contains(id))
}

/// The ids of some attempts.
fn got_ids[P](got: List[TaskAttempt[P]]) -> Set[String] {
  set(got.map(a -> a.task.id))
}

/// The ids of the first `i` results.
fn done_ids[P, E](results: List[TaskResult[P, E]], i: Int) -> Set[String] {
  set(results[..i].map(r -> task_of(r).id))
}

/// The first `i` results are recorded (`settled`).
fn settled_upto[P, E](
  before: Dict[TaskId, Entry[P]],
  after: Dict[TaskId, Entry[P]],
  results: List[TaskResult[P, E]],
  i: Int,
) -> Bool {
  all(results[..i], r -> settled(before, after, r))
}

/// The first `i` ids.
fn id_set(ids: List[TaskId], i: Int) -> Set[String] {
  set(ids[..i])
}

/// The first `i` ids given back (`unlocked`).
fn unlocked_upto[P](
  before: Dict[TaskId, Entry[P]],
  after: Dict[TaskId, Entry[P]],
  ids: List[TaskId],
  i: Int,
) -> Bool {
  all(ids[..i], id -> unlocked(before, after, id))
}

/// The ids of no task.
lemma ids_none[P](tasks: List[Task[P]], s: Set[String])
  requires (s == Set[String] {})
  ensures s == ids_upto(tasks, 0) {
  unfold(ids_upto)
  match tasks {
    [] => simplify(),
    [_, ..rest] => simplify(),
  }
}

/// The ids of one more task.
/// The ids of a task list with one more task at the end.
lemma map_snoc_id[P](ts: List[Task[P]], t: Task[P])
  ensures (ts + [t]).map(t1 -> t1.id) == ts.map(t1 -> t1.id) + [t.id] {
  match ts {
    [] => simplify(),
    [y, ..rest] => simplify(),
  }
}

lemma ids_step[P](tasks: List[Task[P]], i: Int)
  requires 0 <= i && i < len(tasks)
  ensures ids_upto(tasks, i + 1) == ids_upto(tasks, i).with(tasks[i].id) {
  unfold(ids_upto)
  simplify(take_snoc(tasks, i))
  simplify(map_snoc_id(take(tasks, i), tasks[i]))
  extensionality()
  simplify(contains_snoc)
}

/// One more id.
/// One more element of a list, taken from the front.
lemma take_snoc[T](xs: List[T], i: Int)
  requires 0 <= i && i < len(xs)
  ensures take(xs, i + 1) == take(xs, i) + [xs[i]] {
  match xs {
    [] => arithmetic(),
    [y, ..rest] => if i == 0 {
      match rest {
        [] => simplify(),
        [_, ..more] => simplify(),
      }
    } else {
      assert i >= 1 by {
        arithmetic()
      }
      assert 0 <= i - 1 && i - 1 < len(rest) by {
        arithmetic()
      }
      assert take(rest, i - 1 + 1) == take(rest, i - 1) + [rest[i - 1]] by {
        take_snoc(rest, i - 1)
      }
      simplify()
      if i <= 0 {
        arithmetic()
      } else {
        if i + 1 <= 0 {
          arithmetic()
        } else {
          simplify()
        }
      }
    },
  }
}

lemma id_set_step(ids: List[TaskId], i: Int)
  requires 0 <= i && i < len(ids)
  ensures id_set(ids, i + 1) == id_set(ids, i).with(ids[i]) {
  unfold(id_set)
  simplify(take_snoc(ids, i))
  extensionality()
  simplify(contains_snoc)
}

/// A pushed attempt adds its id.
/// A contains over a list with one more element at the end.
lemma contains_snoc[T](xs: List[T], x: T, k: T)
  ensures contains(xs + [x], k) == (k == x || contains(xs, k)) {
  match xs {
    [] => simplify(),
    [y, ..rest] => {
      simplify(contains_snoc(rest, x, k))
      auto()
    }
  }
}

/// The ids of an attempt list with one more attempt at the end.
lemma map_snoc[P](got: List[TaskAttempt[P]], a: TaskAttempt[P])
  ensures (got + [a]).map(a1 -> a1.task.id) == got.map(a1 -> a1.task.id) + [a.task.id] {
  match got {
    [] => simplify(),
    [y, ..rest] => simplify(),
  }
}

lemma got_step[P](got: List[TaskAttempt[P]], a: TaskAttempt[P])
  ensures got_ids(got + [a]) == got_ids(got).with(a.task.id) {
  extensionality()
  simplify(map_snoc(got, a))
  simplify(contains_snoc)
}

/// The ids of one more result.
/// The ids of a result list with one more result at the end.
lemma map_snoc_res[P, E](rs: List[TaskResult[P, E]], r: TaskResult[P, E])
  ensures (rs + [r]).map(r1 -> task_of(r1).id) == rs.map(r1 -> task_of(r1).id) + [task_of(r).id] {
  match rs {
    [] => simplify(),
    [y, ..rest] => simplify(),
  }
}

lemma done_step[P, E](results: List[TaskResult[P, E]], i: Int)
  requires 0 <= i && i < len(results)
  ensures done_ids(results, i + 1) == done_ids(results, i).with(task_of(results[i]).id) {
  unfold(done_ids)
  simplify(take_snoc(results, i))
  simplify(map_snoc_res(take(results, i), results[i]))
  extensionality()
  simplify(contains_snoc)
}

/// Taking every element of a list gives the list.
lemma take_all[T](xs: List[T])
  ensures take(xs, len(xs)) == xs {
  match xs {
    [] => simplify(),
    [y, ..rest] => {
      assert take(rest, len(rest)) == rest by {
        take_all(rest)
      }
      simplify()
      if len(rest) + 1 <= 0 {
        arithmetic()
      } else {
        arithmetic()
      }
    },
  }
}

/// A frame over every task's id.
lemma frame_ids[P](
  a: Dict[TaskId, Entry[P]],
  b: Dict[TaskId, Entry[P]],
  tasks: List[Task[P]],
  seen: Set[String],
)
  requires seen == ids_upto(tasks, len(tasks))
  requires a.excluding(seen) == b.excluding(seen)
  ensures a.excluding(tasks.map(t -> t.id)) == b.excluding(tasks.map(t -> t.id)) {
  assert ids_upto(tasks, len(tasks)) == set(tasks.map(t -> t.id)) by {
    unfold(ids_upto)
    simplify(take_all(tasks))
  }
  calc {
    a.excluding(set(tasks.map(t -> t.id))) == a.excluding(seen);
    _ == b.excluding(seen);
    _ == b.excluding(set(tasks.map(t -> t.id)));
  }
}

/// The frame by the attempts' ids, from the frame by their set.
lemma frame_got[P](
  a: Dict[TaskId, Entry[P]],
  b: Dict[TaskId, Entry[P]],
  got: List[TaskAttempt[P]],
  seen: Set[String],
)
  requires seen == got_ids(got)
  requires a.excluding(seen) == b.excluding(seen)
  ensures a.excluding(got.map(x -> x.task.id)) == b.excluding(got.map(x -> x.task.id)) {
  calc {
    a.excluding(set(got.map(x -> x.task.id))) == a.excluding(seen);
    _ == b.excluding(seen);
    _ == b.excluding(set(got.map(x -> x.task.id)));
  }
}

/// The frame by the ids, from the agreement outside their set.
lemma outside_ids[P](
  a: Dict[TaskId, Entry[P]],
  b: Dict[TaskId, Entry[P]],
  ids: List[TaskId],
)
  requires a.excluding(id_set(ids, len(ids))) == b.excluding(id_set(ids, len(ids)))
  ensures a.excluding(ids) == b.excluding(ids) {
  assert id_set(ids, len(ids)) == set(ids) by {
    unfold(id_set)
    simplify(take_all(ids))
  }
  calc {
    a.excluding(set(ids)) == a.excluding(id_set(ids, len(ids)));
    _ == b.excluding(id_set(ids, len(ids)));
    _ == b.excluding(set(ids));
  }
}

/// The frame by the results' ids, from the agreement outside their set.
lemma outside_done[P, E](
  a: Dict[TaskId, Entry[P]],
  b: Dict[TaskId, Entry[P]],
  results: List[TaskResult[P, E]],
)
  requires a.excluding(done_ids(results, len(results))) == b.excluding(done_ids(results, len(results)))
  ensures a.excluding(ids(results)) == b.excluding(ids(results)) {
  unfold(ids)
  assert done_ids(results, len(results)) == set(results.map(r -> task_of(r).id)) by {
    unfold(done_ids)
    simplify(take_all(results))
  }
  calc {
    a.excluding(set(results.map(r -> task_of(r).id))) == a.excluding(done_ids(results, len(results)));
    _ == b.excluding(done_ids(results, len(results)));
    _ == b.excluding(set(results.map(r -> task_of(r).id)));
  }
}

/// An id pushed onto the order keeps the table listed.
lemma listed_push[P](
  table: Dict[TaskId, Entry[P]],
  order: List[TaskId],
  k: TaskId,
)
  requires listed(table, order)
  ensures listed(table, order + [k]) {
  unfold(listed)
  simplify(contains_snoc)
  assume(k1)
  match table(k1) {
    Some(v) => {
      simplify(listed)
      auto()
    },
    None => simplify(),
  }
}

/// A task new to `d` among the first `i + 1`.
/// `any` over a list with one more task at the end.
lemma any_snoc_new[P](
  d: Dict[TaskId, Entry[P]],
  ts: List[Task[P]],
  t: Task[P],
)
  ensures any(ts + [t], t1 -> !d.contains(t1.id)) == (any(ts, t1 -> !d.contains(t1.id)) || !d.contains(t.id)) {
  match ts {
    [] => simplify(),
    [y, ..rest] => {
      simplify(any_snoc_new(d, rest, t))
      auto()
    },
  }
}

lemma some_new_here[P](
  d: Dict[TaskId, Entry[P]],
  tasks: List[Task[P]],
  i: Int,
)
  requires 0 <= i && i < len(tasks)
  requires !d.contains(tasks[i].id)
  ensures some_new(d, tasks, i + 1) {
  unfold(some_new)
  simplify(take_snoc(tasks, i))
  assert any(take(tasks, i) + [tasks[i]], t -> !d.contains(t.id)) == (any(take(tasks, i), t -> !d.contains(t.id)) || !d.contains(tasks[i].id)) by {
    any_snoc_new(d, take(tasks, i), tasks[i])
  }
  auto()
}

/// Still some new task, or nothing added.
lemma some_new_kept[P](
  d: Dict[TaskId, Entry[P]],
  tasks: List[Task[P]],
  i: Int,
  added: Int,
)
  requires 0 <= i && i < len(tasks)
  requires added > 0 ==> some_new(d, tasks, i)
  ensures added > 0 ==> some_new(d, tasks, i + 1) {
  unfold(some_new)
  simplify(take_snoc(tasks, i))
  assert any(take(tasks, i) + [tasks[i]], t -> !d.contains(t.id)) == (any(take(tasks, i), t -> !d.contains(t.id)) || !d.contains(tasks[i].id)) by {
    any_snoc_new(d, take(tasks, i), tasks[i])
  }
  auto()
}

/// A write under a key no recorded result names keeps them recorded.
lemma settled_insert[P, E](
  before: Dict[TaskId, Entry[P]],
  after: Dict[TaskId, Entry[P]],
  results: List[TaskResult[P, E]],
  i: Int,
  k: TaskId,
  e: Entry[P],
)
  requires 0 <= i && i <= len(results)
  requires settled_upto(before, after, results, i)
  requires !done_ids(results, i).contains(k)
  ensures settled_upto(before, after.with(k, e), results, i) {
  ?proof
}

/// A write under a key not among the first `i` keeps them given back.
lemma unlocked_insert[P](
  before: Dict[TaskId, Entry[P]],
  after: Dict[TaskId, Entry[P]],
  ids: List[TaskId],
  i: Int,
  k: TaskId,
  e: Entry[P],
)
  requires 0 <= i && i <= len(ids)
  requires unlocked_upto(before, after, ids, i)
  requires !id_set(ids, i).contains(k)
  ensures unlocked_upto(before, after.with(k, e), ids, i) {
  ?proof
}

/// A write under a key no attempt names keeps them locked.
lemma locked_insert[P](
  before: Dict[TaskId, Entry[P]],
  after: Dict[TaskId, Entry[P]],
  got: List[TaskAttempt[P]],
  by: WorkerId,
  until: Instant,
  k: TaskId,
  e: Entry[P],
)
  requires all_locked(before, after, got, by, until)
  requires !got_ids(got).contains(k)
  ensures all_locked(before, after.with(k, e), got, by, until) {
  ?proof
}

/// A pushed attempt with a new id keeps the ids unique.
/// `contains_snoc` with the new element on the left of the equality.
lemma contains_snoc_flip[T](xs: List[T], x: T, k: T)
  ensures contains(xs + [x], k) == (x == k || contains(xs, k)) {
  match xs {
    [] => {
      simplify()
      auto()
    },
    [y, ..rest] => {
      simplify(contains_snoc(rest, x, k))
      auto()
    },
  }
}

/// A unique list with one new element at the end stays unique.
lemma unique_snoc[T](xs: List[T], x: T)
  requires xs.unique()
  requires !xs.contains(x)
  ensures (xs + [x]).unique() {
  match xs {
    [] => simplify(),
    [y, ..rest] => {
      assert rest.unique() by {
        auto()
      }
      assert !rest.contains(x) by {
        auto()
      }
      assert (rest + [x]).unique() by {
        unique_snoc(rest, x)
      }
      assert !rest.contains(y) by {
        auto()
      }
      assert !(x == y) by {
        auto()
      }
      simplify(contains_snoc_flip(rest, x, y))
      auto()
    },
  }
}

lemma unique_push[P](got: List[TaskAttempt[P]], a: TaskAttempt[P])
  requires got.map(x -> x.task.id).unique()
  requires !got_ids(got).contains(a.task.id)
  ensures (got + [a]).map(x -> x.task.id).unique() {
  simplify(map_snoc(got, a))
  unique_snoc(got.map(x -> x.task.id), a.task.id)
}

