fn a(x: Int) -> Int
  ensures it > x {
  x + 1
}

type Holder = { it: Iterator[Int] }

fn units(s: String) -> Natural {
  h := Holder { it: [byte_len(s)].iter() }
  mut n: Natural = 0
  for _ in h.it {
    n += 1
  }
  n
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
