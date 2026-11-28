// Goal: compiler/new.strata's `taken` meets its `ensures`, which #677 no longer proves
// (its loop invariant: an `all` whose element fact is an `any`).

/// The names of `given` that `existing` has.
pub fn taken(existing: List[String], given: List[String]) -> List[String]
  ensures it.all(n -> contains(existing, n)) {
  mut out := []
  for n in given {
    if contains(existing, n) {
      mut out.push(n)
    }
  }
  out
}

/// `taken` meets its `ensures`.
lemma and_assoc(a: Bool, b: Bool, c: Bool)
  ensures (a && (b && c)) == ((a && b) && c) {
  if a {
    auto()
  } else {
    auto()
  }
}

lemma all_append[T](xs: List[T], ys: List[T], p: (T) -> Bool)
  ensures all(xs + ys, p) == (all(xs, p) && all(ys, p)) {
  match xs {
    [] => simplify(),
    [y, ..rest] => simplify(all_append(rest, ys, p), and_assoc),
  }
}

lemma taken_fold(existing: List[String], given: List[String], out: List[String])
  requires all(out, n -> contains(existing, n))
  ensures all(fold(given, out, (o, n) -> if contains(existing, n) { o + [n] } else { o }), n -> contains(existing, n)) {
  match given {
    [] => simplify(),
    [g, ..rest] => if contains(existing, g) {
      assert all(out + [g], n -> contains(existing, n)) by { simplify(all_append) }
      simplify(taken_fold(existing, rest, out + [g]))
    } else {
      simplify(taken_fold(existing, rest, out))
    },
  }
}

lemma taken_ensures() {
  unfold(taken)
  taken_fold(existing, given, [])
}
