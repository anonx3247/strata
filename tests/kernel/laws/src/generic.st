// Bounded generics: `[A: Ord]` is one dictionary argument (`lt_A`) with
// Ord's laws (`irreflexive`, `transitive`, `total`) as hypotheses, so a
// law proven here holds for every `Ord`.

/// Each element at most the next, by `A`'s order.
fn is_sorted_by[A: Ord](xs: List[A]) -> Bool {
  match xs {
    [] => true,
    [x, ..rest] => match rest {
      [] => true,
      [y, ..more] => x <= y && is_sorted_by(rest),
    },
  }
}

/// How many times `x` is in `xs`.
fn count_of[A](x: A, xs: List[A]) -> Int {
  match xs {
    [] => 0,
    [y, ..rest] => if x == y {
      1 + count_of(x, rest)
    } else {
      count_of(x, rest)
    },
  }
}

lemma sorted_is_sorted_by[A: Ord](xs: List[A]) ensures is_sorted_by(xs.sorted()) {
  ?proof
}

lemma sorted_count_of[A: Ord](x: A, xs: List[A]) ensures count_of(x, xs.sorted()) == count_of(x, xs) {
  ?proof
}

lemma sorted_len_of[A: Ord](xs: List[A]) ensures len(xs.sorted()) == len(xs) {
  ?proof
}
