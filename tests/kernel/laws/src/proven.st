// proven: laws proven in the Strata proof syntax (docs/kernel-laws.md), one
// or more per proof step: induction as a recursive `match`, `if`,
// `simplify`, `simplify_once`, `unfold`, `assert … by`, `calc`, `witness`,
// `arithmetic`, a law used by calling it, and the ladder.

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

type Tree = | Leaf(Int) | Node(List[Tree])

fn leaves(t: Tree) -> Int {
  match t {
    Leaf(_) => 1,
    Node(ts) => leaves_all(ts),
  }
}

fn leaves_all(ts: List[Tree]) -> Int {
  match ts {
    [] => 0,
    [t, ..rest] => leaves(t) + leaves_all(rest),
  }
}

fn double(n: Int) -> Int = n + n

lemma append_nil[T](xs: List[T]) ensures xs + [] == xs {
  match xs {
    [] => simplify(),
    [_, ..rest] => simplify(append_nil(rest)),
  }
}

lemma append_assoc[T](a: List[T], b: List[T], c: List[T]) ensures (a + b) + c == a + (b + c) {
  match a {
    [] => simplify(),
    [_, ..rest] => simplify(append_assoc(rest, b, c)),
  }
}

lemma reversed_append[T](a: List[T], b: List[T]) ensures (a + b).reversed() == b.reversed() + a.reversed() {
  match a {
    [] => simplify(append_nil),
    [_, ..rest] => simplify(reversed_append(rest, b), append_assoc),
  }
}

lemma reversed_reversed[T](xs: List[T]) ensures xs.reversed().reversed() == xs {
  match xs {
    [] => simplify(),
    [_, ..rest] => simplify(reversed_append, reversed_reversed(rest), append_nil),
  }
}

lemma len_append[T](a: List[T], b: List[T]) ensures len(a + b) == len(a) + len(b) {
  match a {
    [] => simplify(),
    [_, ..rest] => simplify(len_append(rest, b)),
  }
}

/// `assert … by`, then `arithmetic`.
lemma longer(a: List[Int], b: List[Int]) requires len(b) > 0 ensures len(a + b) > len(a) {
  assert len(a + b) == len(a) + len(b) by {
    len_append(a, b)
  }
  arithmetic()
}

/// The ladder: no proof.
lemma len_nonneg[T](xs: List[T]) ensures len(xs) >= 0

/// `if` splits on a condition.
lemma absolute(x: Int) ensures 0 <= (if x < 0 { 0 - x } else { x }) {
  if x < 0 {
    arithmetic()
  } else {
    arithmetic()
  }
}

/// `unfold`, then `arithmetic`.
lemma double_more(n: Int) requires n > 0 ensures double(n) > n {
  unfold(double)
  arithmetic()
}

/// `calc`, each step by the ladder, which uses the facts in scope.
lemma calc_steps[T](xs: List[T]) ensures (xs + []) + [] == xs {
  assert xs + [] == xs by {
    append_nil(xs)
  }
  calc {
    (xs + []) + [] == xs + [];
    _ == xs
  }
}

/// `simplify_once`.
lemma once_step[T](xs: List[T]) ensures (xs + []).reversed() == xs.reversed() {
  simplify_once(append_nil(xs))
}

/// `witness`.
lemma some_longer(xs: List[Int]) ensures exists(n -> n > len(xs)) {
  witness(len(xs) + 1)
  arithmetic()
}

/// Mutual induction: `leaves_pos` and `leaves_all_pos` call each other on
/// the smaller values.
lemma leaves_pos(t: Tree) ensures leaves(t) >= 0 {
  match t {
    Leaf(_) => simplify(),
    Node(ts) => simplify(leaves_all_pos(ts)),
  }
}

lemma leaves_all_pos(ts: List[Tree]) ensures leaves_all(ts) >= 0 {
  match ts {
    [] => simplify(),
    [t, ..rest] => {
      assert leaves(t) >= 0 by {
        leaves_pos(t)
      }
      assert leaves_all(rest) >= 0 by {
        leaves_all_pos(rest)
      }
      simplify()
      arithmetic()
    },
  }
}

// ---- sort over any `Ord` gives a sorted permutation ----

/// A law that holds `!p` assumes `p` and proves `false`.
lemma ord_asym[A: Ord](a: A, b: A) requires a < b ensures !(b < a) {
  irreflexive(a)
  transitive(a, b, a)
}

lemma sorted_two[A: Ord](y: A, z: A, zs: List[A]) ensures is_sorted_by([y, z] + zs) == (y <= z && is_sorted_by([z] + zs)) {
  simplify()
}

lemma insert_sorted_sorted[A: Ord](x: A, xs: List[A]) requires is_sorted_by(xs) ensures is_sorted_by(insert_sorted(x, xs)) {
  match xs {
    [] => simplify(),
    [y, ..ys] => if x <= y {
      simplify()
    } else {
      match ys {
        [] => simplify(ord_asym(y, x)),
        [z, ..zs] => {
          assert is_sorted_by([z] + zs) by {
            simplify(sorted_two)
          }
          assert is_sorted_by(insert_sorted(x, [z] + zs)) by {
            insert_sorted_sorted(x, [z] + zs)
          }
          if x <= z {
            simplify(sorted_two, ord_asym(y, x))
          } else {
            simplify(sorted_two)
          }
        },
      }
    },
  }
}

lemma sorted_is_sorted_by[A: Ord](xs: List[A]) ensures is_sorted_by(xs.sorted()) {
  match xs {
    [] => simplify(),
    [x, ..rest] => {
      assert is_sorted_by(rest.sorted()) by {
        sorted_is_sorted_by(rest)
      }
      simplify()
      insert_sorted_sorted(x, rest.sorted())
    },
  }
}

lemma count_insert[A: Ord](z: A, x: A, xs: List[A]) ensures count_of(z, insert_sorted(x, xs)) == (if z == x { 1 } else { 0 }) + count_of(z, xs) {
  match xs {
    [] => simplify(),
    [y, ..ys] => if x <= y {
      simplify()
    } else {
      simplify(count_insert(z, x, ys))
    },
  }
}

lemma sorted_count_of[A: Ord](z: A, xs: List[A]) ensures count_of(z, xs.sorted()) == count_of(z, xs) {
  match xs {
    [] => simplify(),
    [x, ..rest] => simplify(count_insert, sorted_count_of(z, rest)),
  }
}
