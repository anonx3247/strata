type Tree[T] =
  | Leaf
  | Node(Tree[T], T, Tree[T])

fn insert(t: Tree[Int], x: Int) -> Tree[Int] {
  match t {
    Leaf => Node(Leaf, x, Leaf),
    Node(l, v, r) => if x < v {
      Node(insert(l, x), v, r)
    } else {
      Node(l, v, insert(r, x))
    },
  }
}

fn size[T](t: Tree[T]) -> Int {
  match t {
    Leaf => 0,
    Node(l, _, r) => size(l) + 1 + size(r),
  }
}

fn main() {
  mut t: Tree[Int] = Leaf
  for i in range(0, 10) {
    t = insert(t, i * 7 % 10)
  }
  print(size(t))
  print(insert(insert(Leaf, 2), 1))
}
