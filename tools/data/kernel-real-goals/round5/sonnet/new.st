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

lemma all_snoc(xs: List[String], x: String, existing: List[String])
  requires all(xs, n -> contains(existing, n))
  requires contains(existing, x)
  ensures all(xs + [x], n -> contains(existing, n)) {
  match xs {
    [] => simplify(),
    [y, ..rest] => {
      assert all(rest + [x], n -> contains(existing, n)) by { all_snoc(rest, x, existing) }
      simplify()
    },
  }
}

lemma taken_acc(existing: List[String], given: List[String], acc: List[String])
  requires all(acc, n -> contains(existing, n))
  ensures all(fold(given, acc, (out, n) -> if contains(existing, n) { out + [n] } else { out }), n -> contains(existing, n)) {
  match given {
    [] => simplify(),
    [y, ..rest] => if contains(existing, y) {
      assert all(acc + [y], n -> contains(existing, n)) by { all_snoc(acc, y, existing) }
      simplify(taken_acc(existing, rest, acc + [y]))
    } else {
      simplify(taken_acc(existing, rest, acc))
    },
  }
}

/// `taken` meets its `ensures`.
lemma taken_ensures() {
  unfold(taken)
  taken_acc(existing, given, [])
}
