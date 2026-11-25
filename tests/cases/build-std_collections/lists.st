type Shape = | Dot | Circle(Int) | Rect { w: Int, h: Int }

type Board = { cells: List[String], moves: Int }

fn main() {
  // compare: the order sort sorts by
  print([compare(1, 2), compare(2, 2), compare(3, 2)])
  print([compare("a", "b"), compare("b", "a"), compare("ab", "a")])
  print([compare((1, "b"), (1, "a")), compare([1, 2], [1, 2, 0])])
  print([compare(Circle(3), Rect { w: 1, h: 1 }), compare(Dot, Circle(0)), compare(Some(1), None)])
  print(compare(100000000000000000000000, 100000000000000000000001))
  print(compare(1.5, -2.0))
  // set_at: a copy when the list is shared, in place when not
  mut xs := ["a", "b", "c"]
  ys := xs
  xs = set_at(xs, 1, "B")
  print((xs, ys))
  xs = set_at(xs, 2, xs[0] + "!")
  zs := set_at(ys, 0, "z")
  print((xs, ys, zs))
  // take in place, then push
  xs = take(xs, 1)
  mut xs.push("d")
  print(xs)
  // a field
  mut b := Board { cells: ["", "", ""], moves: 0 }
  for i in 0..3 {
    b.cells = set_at(b.cells, i, f"x{i}")
    b.moves += 1
  }
  print(b)
  // a table filled in place: Fibonacci numbers
  mut fib := [0 for _ in 0..90]
  fib = set_at(fib, 1, 1)
  for i in 2..90 {
    fib = set_at(fib, i, fib[i - 1] + fib[i - 2])
  }
  print(fib[89])
}
