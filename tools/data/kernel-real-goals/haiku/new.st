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

/// Adding a member that `existing` has keeps `all` of the list.
lemma all_snoc_in(acc: List[String], y: String, existing: List[String])
  requires contains(existing, y)
  requires all(acc, n -> contains(existing, n))
  ensures all(acc + [y], n -> contains(existing, n)) {
  match acc {
    [] => simplify(),
    [z, ..rest] => {
      assert all(rest, n -> contains(existing, n)) by {
        auto()
      }
      assert all(rest + [y], n -> contains(existing, n)) by {
        all_snoc_in(rest, y, existing)
      }
      auto()
    },
  }
}

/// The fold keeps every member of `existing`.
lemma fold_members(existing: List[String], given: List[String], acc: List[String])
  requires all(acc, n -> contains(existing, n))
  ensures all(fold(given, acc, out -> n -> if contains(existing, n) { out + [n] } else { out }), n -> contains(existing, n)) {
  match given {
    [] => simplify(),
    [y, ..rest] => {
      if contains(existing, y) {
        assert all(acc + [y], n -> contains(existing, n)) by {
          all_snoc_in(acc, y, existing)
        }
        simplify()
      } else {
        simplify()
      }
    },
  }
}

/// `taken` meets its `ensures`.
lemma taken_ensures() {
  assert all([], n -> contains(existing, n)) by {
    simplify()
  }
  unfold(taken)
  fold_members(existing, given, [])
}
