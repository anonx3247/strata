fn a(x: Int) -> Int
  ensures it > x {
  x + 1
}

fn units(s: String) -> Natural {
  len(s.graphemes())
}

fn b(x: Int) -> Int
  ensures it >= 0 {
  x * x
}

fn c(xs: List[Int]) -> Int {
  len(xs)
}

fn sq(x: Float) -> Float
  ensures it >= 0.0 {
  x * x
}
