// lists: laws about std's list functions, whose definitions in the logic
// are their models in core/models.strata.

/// Each element at most the next.
fn is_sorted(xs: List[Int]) -> Bool {
  match xs {
    [] => true,
    [x, ..rest] => match rest {
      [] => true,
      [y, ..more] => x <= y && is_sorted(rest),
    },
  }
}

/// How many times `x` is in `xs`.
fn count(x: Int, xs: List[Int]) -> Int {
  match xs {
    [] => 0,
    [y, ..rest] => if x == y {
      1 + count(x, rest)
    } else {
      count(x, rest)
    },
  }
}

/// A contract: its `ensures` is the goal `twice.ensures`.
fn twice[A](xs: List[A]) -> Int ensures it == 2 * len(xs) {
  len(xs + xs)
}

/// Proven: `[]` is `+`'s unit on the right.
lemma append_nil[T](xs: List[T]) ensures xs + [] == xs {
  match xs {
    [] => simplify(),
    [_, ..rest] => simplify(append_nil(rest)),
  }
}

/// Proven: `+` is associative.
lemma append_assoc[T](a: List[T], b: List[T], c: List[T]) ensures (a + b) + c == a + (b + c) {
  match a {
    [] => simplify(),
    [_, ..rest] => simplify(append_assoc(rest, b, c)),
  }
}

/// Proven.
lemma reversed_append[T](a: List[T], b: List[T]) ensures (a + b).reversed() == b.reversed() + a.reversed() {
  match a {
    [] => simplify(append_nil),
    [_, ..rest] => simplify(reversed_append(rest, b), append_assoc),
  }
}

/// Proven: `reversed` is an involution.
lemma reversed_reversed[T](xs: List[T]) ensures xs.reversed().reversed() == xs {
  match xs {
    [] => simplify(),
    [_, ..rest] => simplify(reversed_append, reversed_reversed(rest), append_nil),
  }
}

lemma len_append[A](xs: List[A], ys: List[A]) ensures len(xs + ys) == len(xs) + len(ys) {
  ?proof
}

lemma len_reversed[A](xs: List[A]) ensures len(xs.reversed()) == len(xs) {
  ?proof
}

lemma map_append[A, B](xs: List[A], ys: List[A], f: (A) -> B) ensures (xs + ys).map(f) == xs.map(f) + ys.map(f) {
  ?proof
}

lemma map_map[A, B, C](xs: List[A], f: (B) -> C, g: (A) -> B) ensures xs.map(g).map(f) == xs.map(x -> f(g(x))) {
  ?proof
}

lemma map_reversed[A, B](xs: List[A], f: (A) -> B) ensures xs.reversed().map(f) == xs.map(f).reversed() {
  ?proof
}

lemma len_sorted(xs: List[Int]) ensures len(xs.sorted()) == len(xs) {
  ?proof
}

lemma sorted_is_sorted(xs: List[Int]) ensures is_sorted(xs.sorted()) {
  ?proof
}

lemma count_sorted(x: Int, xs: List[Int]) ensures count(x, xs.sorted()) == count(x, xs) {
  ?proof
}

/// The goal `twice.ensures`.
lemma twice_ensures() {
  ?proof
}

/// Refinements: `len` returns a `Natural`, an assumed fact.
lemma len_nonneg[A](xs: List[A]) ensures len(xs) >= 0 {
  ?proof
}

lemma len_sorted_nonneg(xs: List[Int]) ensures len(xs.sorted()) >= 0 {
  ?proof
}

lemma natural_add(a: Natural, b: Natural) ensures a + b >= a {
  ?proof
}
