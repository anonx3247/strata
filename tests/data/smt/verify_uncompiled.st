fn a(x: Int) -> Int
  ensures it > x {
  x + 1
}

fn units(s: String) -> Natural {
  mut n := 0
  comptime for k in [1, 2] {
    n += k
  }
  n + byte_len(s)
}

fn longer(s: String) -> Natural
  ensures it >= 1 {
  units(s) + 1
}

fn longest(s: String, t: String) -> Natural {
  max(longer(s), longer(t))
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
