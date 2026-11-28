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

/// `all` over an append.
lemma all_append(xs: List[String], ys: List[String], p: (String) -> Bool)
  ensures all(xs + ys, p) == (all(xs, p) && all(ys, p)) {
  match xs {
    [] => simplify(),
    [y, ..rest] => simplify(all_append(rest, ys, p)),
  }
}

/// The loop of `taken`, from any list `out` that already meets the property.
lemma taken_fold(given: List[String], out: List[String], existing: List[String])
  requires all(out, n -> contains(existing, n))
  ensures all(fold(given, out, o -> n -> if contains(existing, n) { o + [n] } else { o }), n -> contains(existing, n)) {
  match given {
    [] => simplify(),
    [y, ..rest] => if contains(existing, y) {
      assert all(out + [y], n -> contains(existing, n)) by {
        auto(all_append)
      }
      simplify()
    } else {
      simplify()
    },
  }
}

/// `taken` meets its `ensures`.
lemma taken_ensures() {
  unfold(taken)
  taken_fold(given, [], existing)
}
