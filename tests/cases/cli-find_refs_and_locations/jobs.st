/// A unit of work.
pub type Task = { id: Int, attempts: Int }

pub type Shape =
  | Circle { r: Float }
  | Dot

/// Something that hands out tasks.
pub type Worker = { queue: List[Task] }

impl Worker {
  /// The next task, if any.
  fn next(self) -> Option[Task] {
    get(self.queue, 0)
  }
}

pub trait Describe {
  fn describe(self) -> String
}

impl Describe for Task {
  fn describe(self) -> String {
    f"task {self.id}"
  }
}

/// The task at `i`.
pub fn get_task(xs: List[Task], i: Int) -> Task {
  xs[i]
}

pub fn retried(t: Task) -> Task {
  mut u := t
  u.attempts = u.attempts + 1
  u
}

pub fn same_ids(a: Task, b: Task) -> Bool {
  a.id == b.id
}

pub fn area(s: Shape) -> Float {
  match s {
    Circle { r } => 3.0 * r * r,
    Dot => 0.0,
  }
}

pub fn run_worker(w: Worker) -> Int {
  mut n := 0
  first := Task { id: 1, attempts: 0 }
  while same_ids(first, get_task(w.queue, 0)) {
    n += 1
    if n > 3 {
      break
    }
  }
  match w.next() {
    Some(t) => t.attempts + n,
    None => n,
  }
}
