// `Eq` and `Ord` laws. `==` is built in for every type (part by part),
// which the logic reads as its equality, so `Eq`'s laws are about it; the
// lab has no derived `Ord`, so these types write `lt` by hand, as a
// derived one would (by variant order, then field by field).

type Priority = | Low | Mid | High

fn rank(p: Priority) -> Int {
  match p {
    Low => 0,
    Mid => 1,
    High => 2,
  }
}

impl Ord for Priority {
  fn lt(self, other: Priority) -> Bool = rank(self) < rank(other)
}

type Task = { priority: Priority, due: Int }

impl Ord for Task {
  fn lt(self, other: Task) -> Bool = self.priority < other.priority || self.priority == other.priority && self.due < other.due
}

lemma priority_eq_refl(p: Priority) ensures p == p {
  ?proof
}

lemma priority_eq_symm(p: Priority, q: Priority) requires p == q ensures q == p {
  ?proof
}

lemma priority_eq_trans(p: Priority, q: Priority, r: Priority) requires p == q requires q == r ensures p == r {
  ?proof
}

lemma priority_lt_irrefl(p: Priority) ensures !(p < p) {
  ?proof
}

lemma priority_lt_trans(p: Priority, q: Priority, r: Priority) requires p < q requires q < r ensures p < r {
  ?proof
}

lemma priority_total(p: Priority, q: Priority) ensures p < q || p == q || q < p {
  ?proof
}

lemma task_eq_symm(s: Task, t: Task) requires s == t ensures t == s {
  ?proof
}

lemma task_lt_irrefl(t: Task) ensures !(t < t) {
  ?proof
}

lemma task_lt_trans(s: Task, t: Task, u: Task) requires s < t requires t < u ensures s < u {
  ?proof
}

lemma task_total(s: Task, t: Task) ensures s < t || s == t || t < s {
  ?proof
}

// each `impl Ord`'s obligations: Ord's laws (core/models.strata, `OrdLaws`)
/// The goal `Priority.irreflexive`.
lemma priority_irreflexive() {
  ?proof
}

/// The goal `Priority.transitive`.
lemma priority_transitive() {
  ?proof
}

/// The goal `Priority.total`.
lemma priority_total() {
  ?proof
}

/// The goal `Task.irreflexive`.
lemma task_irreflexive() {
  ?proof
}

/// The goal `Task.transitive`.
lemma task_transitive() {
  ?proof
}

/// The goal `Task.total`.
lemma task_total() {
  ?proof
}
