type Tree =
  | Leaf
  | Node { left: Tree, value: Int where it >= 0, right: Tree }

fn sum(t: Tree) -> Int {
  match t {
    Leaf => 0,
    Node { left, value, right } => sum(left) + value + sum(right),
  }
}

/// A tree's sum is never negative: by induction on the tree, its
/// recursive calls giving the fact for the subtrees.
lemma sum_natural(t: Tree)
  ensures sum(t) >= 0 {
  match t {
    Leaf => (),
    Node { left, right, .. } => {
      sum_natural(left)
      sum_natural(right)
    },
  }
}

fn total_plain(ts: List[Tree]) -> Int where it >= 0 {
  mut s := 0
  mut i := 0
  while i < len(ts)
    invariant s >= 0 {
    s += sum(ts[i])
    i += 1
  }
  s
}

fn total(ts: List[Tree]) -> Int where it >= 0 {
  mut s := 0
  mut i := 0
  while i < len(ts)
    invariant s >= 0 {
    t := ts[i]
    assert sum(t) >= 0 by sum_natural(t)
    s += sum(t)
    i += 1
  }
  s
}

fn main() {
  print(total([Node { left: Leaf, value: 2, right: Leaf }]))
}
