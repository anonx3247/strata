// `mut xs.grow(n)`: room for n more elements; the list's value never changes
type Box = { items: List[String] }

/// A list grown and pushed onto: the same value as without `grow`.
fn filled(n: Int where it >= 0) -> List[Int]
  ensures len(it) == n {
  mut xs: List[Int] = []
  mut xs.grow(n)
  mut i := 0
  while i < n
    invariant len(xs) == i {
    mut xs.push(i)
    i += 1
  }
  xs
}

/// `grow` changes no value.
fn same(xs: List[Int], n: Int where it >= 0) -> List[Int]
  ensures it == xs {
  mut ys := xs
  mut ys.grow(n)
  ys
}

fn main() {
  print(filled(5))
  a := [1, 2, 3]
  mut b := a
  mut b.grow(10)
  mut b.push(4)
  print((a, b))
  mut bx := Box { items: ["x"] }
  mut bx.items.grow(3)
  mut bx.items.push("y")
  print(bx)
  mut e: List[String] = []
  mut e.grow(0)
  print((e, same([7], 2)))
}

/// A size that may be negative: `grow`'s `requires` is refuted.
fn negative(xs: List[Int], n: Int) -> List[Int] {
  mut ys := xs
  mut ys.grow(n)
  ys
}
