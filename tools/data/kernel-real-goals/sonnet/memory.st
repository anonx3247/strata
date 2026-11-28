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
  match tasks {
    [] => simplify(),
    [y, ..rest] => simplify(),
  }
}

/// The ids of one more task.
lemma ids_step[P](tasks: List[Task[P]], i: Int)
  requires 0 <= i && i < len(tasks)
  ensures ids_upto(tasks, i + 1) == ids_upto(tasks, i).with(tasks[i].id) {
  extensionality()
  simplify(take_step(tasks, i), map_append, contains_append)
  or_comm()
}

/// One more id.
lemma id_set_step(ids: List[TaskId], i: Int)
  requires 0 <= i && i < len(ids)
  ensures id_set(ids, i + 1) == id_set(ids, i).with(ids[i]) {
  extensionality()
  simplify(take_step(ids, i), contains_append)
  or_comm()
}

/// A pushed attempt adds its id.
lemma got_step[P](got: List[TaskAttempt[P]], a: TaskAttempt[P])
  ensures got_ids(got + [a]) == got_ids(got).with(a.task.id) {
  extensionality()
  simplify(map_append, contains_append)
  or_comm()
}

/// The ids of one more result.
lemma done_step[P, E](results: List[TaskResult[P, E]], i: Int)
  requires 0 <= i && i < len(results)
  ensures done_ids(results, i + 1) == done_ids(results, i).with(task_of(results[i]).id) {
  extensionality()
  simplify(take_step(results, i), map_append, contains_append)
  or_comm()
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
  assert seen == set(tasks.map(t -> t.id)) by { simplify(take_all(tasks)) }
  assert a.excluding(seen) == b.excluding(seen) ==> a.excluding(tasks.map(t -> t.id)) == b.excluding(tasks.map(t -> t.id)) by {
    simplify_once(fact)
    assume()
    assumption()
  }
  fact1()
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
  assert seen == set(got.map(x -> x.task.id)) by { simplify() }
  assert a.excluding(seen) == b.excluding(seen) ==> a.excluding(got.map(x -> x.task.id)) == b.excluding(got.map(x -> x.task.id)) by {
    simplify_once(fact)
    assume()
    assumption()
  }
  fact1()
}

/// The frame by the ids, from the agreement outside their set.
lemma outside_ids[P](
  a: Dict[TaskId, Entry[P]],
  b: Dict[TaskId, Entry[P]],
  ids: List[TaskId],
)
  requires a.excluding(id_set(ids, len(ids))) == b.excluding(id_set(ids, len(ids)))
  ensures a.excluding(ids) == b.excluding(ids) {
  assert id_set(ids, len(ids)) == set(ids) by { simplify(take_all(ids)) }
  assert a.excluding(id_set(ids, len(ids))) == b.excluding(id_set(ids, len(ids))) ==> a.excluding(ids) == b.excluding(ids) by {
    simplify_once(fact)
    assume()
    assumption()
  }
  fact1()
}

/// The frame by the results' ids, from the agreement outside their set.
lemma outside_done[P, E](
  a: Dict[TaskId, Entry[P]],
  b: Dict[TaskId, Entry[P]],
  results: List[TaskResult[P, E]],
)
  requires a.excluding(done_ids(results, len(results))) == b.excluding(done_ids(results, len(results)))
  ensures a.excluding(ids(results)) == b.excluding(ids(results)) {
  assert done_ids(results, len(results)) == set(ids(results)) by { simplify(take_all(results)) }
  assert a.excluding(done_ids(results, len(results))) == b.excluding(done_ids(results, len(results))) ==> a.excluding(ids(results)) == b.excluding(ids(results)) by {
    simplify_once(fact)
    assume()
    assumption()
  }
  fact1()
}

/// An id pushed onto the order keeps the table listed.
lemma listed_push[P](
  table: Dict[TaskId, Entry[P]],
  order: List[TaskId],
  k: TaskId,
)
  requires listed(table, order)
  ensures listed(table, order + [k]) {
  simplify()
  assume(j)
  match table(j) {
    Some(v) => {
      assert contains(order, j) by { listed_get(table, order, j, v) }
      simplify(contains_append)
    },
    None => simplify(),
  }
}

/// A task new to `d` among the first `i + 1`.
lemma some_new_here[P](
  d: Dict[TaskId, Entry[P]],
  tasks: List[Task[P]],
  i: Int,
)
  requires 0 <= i && i < len(tasks)
  requires !d.contains(tasks[i].id)
  ensures some_new(d, tasks, i + 1) {
  simplify(take_step(tasks, i), any_append)
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
  simplify(take_step(tasks, i), any_append)
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
  assert settled_upto(before, after, results, i) ==> all(take(results, i), r -> settled(before, after, r)) by { simplify() }
  assert all(take(results, i), r -> settled(before, after, r)) by { fact() }
  assert !done_ids(results, i).contains(k) ==> !contains(take(results, i).map(r -> task_of(r).id), k) by { simplify() }
  assert !contains(take(results, i).map(r -> task_of(r).id), k) by { fact2() }
  assert all(take(results, i), r -> settled(before, after.with(k, e), r)) by { settled_all(before, after, take(results, i), k, e) }
  simplify()
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
  assert unlocked_upto(before, after, ids, i) ==> all(take(ids, i), id -> unlocked(before, after, id)) by { simplify() }
  assert all(take(ids, i), id -> unlocked(before, after, id)) by { fact() }
  assert !id_set(ids, i).contains(k) ==> !contains(take(ids, i), k) by { simplify() }
  assert !contains(take(ids, i), k) by { fact2() }
  assert all(take(ids, i), id -> unlocked(before, after.with(k, e), id)) by { unlocked_all(before, after, take(ids, i), k, e) }
  simplify()
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
  assert all_locked(before, after, got, by, until) ==> all(got, a -> locked(before, after, a, by, until)) by { simplify() }
  assert all(got, a -> locked(before, after, a, by, until)) by { fact() }
  assert !got_ids(got).contains(k) ==> !contains(got.map(x -> x.task.id), k) by { simplify() }
  assert !contains(got.map(x -> x.task.id), k) by { fact2() }
  assert all(got, a -> locked(before, after.with(k, e), a, by, until)) by { locked_all(before, after, got, by, until, k, e) }
  simplify()
}

/// A pushed attempt with a new id keeps the ids unique.
lemma unique_push[P](got: List[TaskAttempt[P]], a: TaskAttempt[P])
  requires got.map(x -> x.task.id).unique()
  requires !got_ids(got).contains(a.task.id)
  ensures (got + [a]).map(x -> x.task.id).unique() {
  assert !got_ids(got).contains(a.task.id) ==> !contains(got.map(x -> x.task.id), a.task.id) by { simplify() }
  assert !contains(got.map(x -> x.task.id), a.task.id) by { fact() }
  assert unique(got.map(x -> x.task.id) + [a.task.id]) by { unique_snoc(got.map(x -> x.task.id), a.task.id) }
  simplify(map_append)
}


lemma take_gen[T](xs: List[T], i: Int, j: Int)
  requires 0 <= i
  requires i < len(xs)
  requires j == i + 1
  ensures take(xs, j) == take(xs, i) + [at(xs, i)] {
  match xs {
    [] => { simplify()
      arithmetic() },
    [y, ..rest] => if i == 0 {
      match rest {
        [] => simplify(),
        [z, ..more] => simplify(),
      }
    } else {
      assert i >= 1 by { arithmetic() }
      assert i - 1 < len(rest) by { arithmetic() }
      assert 0 <= i - 1 by { arithmetic() }
      assert take(rest, i) == take(rest, i - 1) + [at(rest, i - 1)] by { take_gen(rest, i - 1, i) }
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

lemma take_step[T](xs: List[T], i: Int)
  requires 0 <= i && i < len(xs)
  ensures take(xs, i + 1) == take(xs, i) + [at(xs, i)] {
  take_gen(xs, i, i + 1)
}

lemma map_append[T, U](xs: List[T], ys: List[T], f: (T) -> U)
  ensures (xs + ys).map(f) == xs.map(f) + ys.map(f) {
  match xs {
    [] => simplify(),
    [y, ..rest] => simplify(map_append(rest, ys, f)),
  }
}

lemma contains_append[T](xs: List[T], ys: List[T], k: T)
  ensures contains(xs + ys, k) == (contains(xs, k) || contains(ys, k)) {
  match xs {
    [] => simplify(),
    [y, ..rest] => simplify(contains_append(rest, ys, k), or_assoc),
  }
}

lemma or_comm(a: Bool, b: Bool)
  ensures (a || b) == (b || a) {
  if a {
    auto()
  } else {
    auto()
  }
}

lemma or_assoc(a: Bool, b: Bool, c: Bool)
  ensures (a || (b || c)) == ((a || b) || c) {
  if a {
    auto()
  } else {
    auto()
  }
}

lemma take_all[T](xs: List[T])
  ensures take(xs, len(xs)) == xs {
  match xs {
    [] => simplify(),
    [y, ..rest] => {
      simplify(take_all(rest))
      if len(rest) + 1 <= 0 {
        arithmetic()
      } else {
        auto()
      }
    },
  }
}

lemma listed_get[P](table: Dict[TaskId, Entry[P]], order: List[TaskId], j: TaskId, v: Entry[P])
  requires all(table.items(), (id, _) -> order.contains(id))
  requires table.get(j) == Some(v)
  ensures order.contains(j) {
  assert all(table.items(), (id, _) -> order.contains(id)) ==> forall(k -> match table.get(k) { Some(w) => order.contains(k), None => true }) by { simplify() }
  assert forall(k -> match table.get(k) { Some(w) => order.contains(k), None => true }) by { fact() }
  assert match table.get(j) { Some(w) => order.contains(j), None => true } by { fact1(j) }
  simplify()
}

lemma any_append[T](xs: List[T], ys: List[T], p: (T) -> Bool)
  ensures any(xs + ys, p) == (any(xs, p) || any(ys, p)) {
  match xs {
    [] => simplify(),
    [y, ..rest] => simplify(any_append(rest, ys, p), or_assoc),
  }
}

lemma settled_one[P, E](
  before: Dict[TaskId, Entry[P]],
  after: Dict[TaskId, Entry[P]],
  r: TaskResult[P, E],
  k: TaskId,
  e: Entry[P],
)
  requires settled(before, after, r)
  requires task_of(r).id != k
  ensures settled(before, after.with(k, e), r) {
  assert settled(before, after, r) ==> settled(before, after.with(k, e), r) by {
    unfold(settled)
    assume()
    match before.get(task_of(r).id) {
      Some(e1) => simplify(),
      None => simplify(),
    }
  }
  fact()
}

lemma settled_all[P, E](
  before: Dict[TaskId, Entry[P]],
  after: Dict[TaskId, Entry[P]],
  xs: List[TaskResult[P, E]],
  k: TaskId,
  e: Entry[P],
)
  requires all(xs, r -> settled(before, after, r))
  requires !contains(xs.map(r -> task_of(r).id), k)
  ensures all(xs, r -> settled(before, after.with(k, e), r)) {
  match xs {
    [] => simplify(),
    [y, ..rest] => {
      assert all(rest, r -> settled(before, after, r)) by { simplify() }
      assert !contains(rest.map(r -> task_of(r).id), k) by { auto() }
      assert !contains(map([y] + rest, r -> task_of(r).id), k) ==> k != task_of(y).id by { simplify() }
      assert k != task_of(y).id by { fact2() }
      assert task_of(y).id != k by {
        if task_of(y).id == k {
          simplify()
        } else {
          auto()
        }
      }
      assert settled(before, after, y) by { auto() }
      assert all(rest, r -> settled(before, after.with(k, e), r)) by { settled_all(before, after, rest, k, e) }
      assert settled(before, after.with(k, e), y) by { settled_one(before, after, y, k, e) }
      simplify()
    },
  }
}

lemma unlocked_one[P](
  before: Dict[TaskId, Entry[P]],
  after: Dict[TaskId, Entry[P]],
  id: TaskId,
  k: TaskId,
  e: Entry[P],
)
  requires unlocked(before, after, id)
  requires id != k
  ensures unlocked(before, after.with(k, e), id) {
  assert unlocked(before, after, id) ==> unlocked(before, after.with(k, e), id) by {
    unfold(unlocked)
    assume()
    match before.get(id) {
      Some(e1) => simplify(),
      None => simplify(),
    }
  }
  fact()
}

lemma unlocked_all[P](
  before: Dict[TaskId, Entry[P]],
  after: Dict[TaskId, Entry[P]],
  xs: List[TaskId],
  k: TaskId,
  e: Entry[P],
)
  requires all(xs, id -> unlocked(before, after, id))
  requires !contains(xs, k)
  ensures all(xs, id -> unlocked(before, after.with(k, e), id)) {
  match xs {
    [] => simplify(),
    [y, ..rest] => {
      assert all(rest, id -> unlocked(before, after, id)) by { simplify() }
      assert !contains(rest, k) by { auto() }
      assert !contains([y] + rest, k) ==> k != y by { simplify() }
      assert k != y by { fact2() }
      assert y != k by {
        if y == k {
          simplify()
        } else {
          auto()
        }
      }
      assert unlocked(before, after, y) by { auto() }
      assert all(rest, id -> unlocked(before, after.with(k, e), id)) by { unlocked_all(before, after, rest, k, e) }
      assert unlocked(before, after.with(k, e), y) by { unlocked_one(before, after, y, k, e) }
      simplify()
    },
  }
}

lemma locked_one[P](
  before: Dict[TaskId, Entry[P]],
  after: Dict[TaskId, Entry[P]],
  a: TaskAttempt[P],
  by: WorkerId,
  until: Instant,
  k: TaskId,
  e: Entry[P],
)
  requires locked(before, after, a, by, until)
  requires a.task.id != k
  ensures locked(before, after.with(k, e), a, by, until) {
  assert locked(before, after, a, by, until) ==> locked(before, after.with(k, e), a, by, until) by {
    unfold(locked)
    assume()
    match before.get(a.task.id) {
      Some(e1) => simplify(),
      None => simplify(),
    }
  }
  fact()
}

lemma locked_all[P](
  before: Dict[TaskId, Entry[P]],
  after: Dict[TaskId, Entry[P]],
  xs: List[TaskAttempt[P]],
  by: WorkerId,
  until: Instant,
  k: TaskId,
  e: Entry[P],
)
  requires all(xs, a -> locked(before, after, a, by, until))
  requires !contains(xs.map(x -> x.task.id), k)
  ensures all(xs, a -> locked(before, after.with(k, e), a, by, until)) {
  match xs {
    [] => simplify(),
    [y, ..rest] => {
      assert all(rest, a -> locked(before, after, a, by, until)) by { simplify() }
      assert !contains(rest.map(x -> x.task.id), k) by { auto() }
      assert !contains(map([y] + rest, x -> x.task.id), k) ==> k != y.task.id by { simplify() }
      assert k != y.task.id by { fact2() }
      assert y.task.id != k by {
        if y.task.id == k {
          simplify()
        } else {
          auto()
        }
      }
      assert locked(before, after, y, by, until) by { auto() }
      assert all(rest, a -> locked(before, after.with(k, e), a, by, until)) by { locked_all(before, after, rest, by, until, k, e) }
      assert locked(before, after.with(k, e), y, by, until) by { locked_one(before, after, y, by, until, k, e) }
      simplify()
    },
  }
}

lemma unique_snoc[T](xs: List[T], x: T)
  requires unique(xs)
  requires !contains(xs, x)
  ensures unique(xs + [x]) {
  match xs {
    [] => simplify(),
    [y, ..rest] => {
      assert unique(rest) by { simplify() }
      assert !contains(rest, x) by { auto() }
      assert !contains([y] + rest, x) ==> x != y by { simplify() }
      assert x != y by { fact2() }
      assert unique(rest + [x]) by { unique_snoc(rest, x) }
      assert y != x by {
        if y == x {
          simplify()
        } else {
          auto()
        }
      }
      simplify(contains_append)
    },
  }
}
