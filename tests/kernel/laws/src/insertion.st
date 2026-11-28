// insertion: insertion sort on `Int` lists, written out, and its laws. A
// list literal can't spread another list in Strata (`[y, ..rest]` is a
// pattern only), so the definitions build lists with `+`.

/// Each element at most the next.
fn is_sorted(xs: List[Int]) -> Bool {
  match xs {
    [] => true,
    [x] => true,
    [x, y, ..rest] => x <= y && is_sorted([y] + rest),
  }
}

/// `x` before the first element it is at most.
fn insert(x: Int, xs: List[Int]) -> List[Int] {
  match xs {
    [] => [x],
    [y, ..rest] => if x <= y {
      [x, y] + rest
    } else {
      [y] + insert(x, rest)
    },
  }
}

fn insertion_sort(xs: List[Int]) -> List[Int] {
  match xs {
    [] => [],
    [x, ..rest] => insert(x, insertion_sort(rest)),
  }
}

/// How many times `v` is in `xs`.
fn count(v: Int, xs: List[Int]) -> Int {
  match xs {
    [] => 0,
    [y, ..rest] => if v == y {
      1 + count(v, rest)
    } else {
      count(v, rest)
    },
  }
}

/// Proven, as the shape shown: `match xs`; `if x <= y`; else `match rest`
/// with a nested `if x <= z`, and the recursive call on `[z, ..more]` in
/// the last branch.
lemma insert_keeps_sorted(x: Int, xs: List[Int]) requires is_sorted(xs) ensures is_sorted(insert(x, xs)) {
  match xs {
    [] => simplify(),
    [y, ..rest] => if x <= y {
      simplify()
    } else {
      match rest {
        [] => auto(),
        [z, ..more] => if x <= z {
          auto()
        } else {
          assert is_sorted(insert(x, [z] + more)) by {
            insert_keeps_sorted(x, [z] + more)
          }
          simplify()
        },
      }
    },
  }
}

lemma sort_is_sorted(xs: List[Int]) ensures is_sorted(insertion_sort(xs)) {
  ?proof
}

lemma sort_keeps_counts(v: Int, xs: List[Int]) ensures count(v, insertion_sort(xs)) == count(v, xs) {
  ?proof
}
