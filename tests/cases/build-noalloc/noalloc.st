// `noalloc` functions: machine integers and floats, reading lists, strings,
// records and variants, loops over a list or a range, and calls of other
// `noalloc` functions, functions and methods; an `assumed` contract (never
// run) may allocate

type Point = { x: I64, y: I64 }

type Shape =
  | Circle { r: Float }
  | Square { side: Float }

trait Area {
  fn area(self) -> Float
    noalloc
}

impl Area for Shape {
  fn area(self) -> Float
    noalloc {
    match self {
      Circle { r } => 3.0 * r * r,
      Square { side } => side * side,
    }
  }
}

impl Point {
  fn manhattan(self) -> I64
    noalloc {
    magnitude(self.x) + magnitude(self.y)
  }
}

fn magnitude(n: I64) -> I64
  noalloc {
  if n < 0.to_i64() {
    0.to_i64() - n
  } else {
    n
  }
}

fn total(xs: List[I64]) -> I64
  assumed ensures it == xs.fold(0.to_i64(), (a, b) -> a + b)
  noalloc {
  mut acc: I64 = 0.to_i64()
  for x in xs {
    acc += x
  }
  acc
}

fn weighted(xs: List[I64]) -> I64
  noalloc {
  mut acc: I64 = 0.to_i64()
  for i in 0..xs.len() {
    acc += i.to_i64() * xs[i]
  }
  acc
}

fn largest(xs: List[I64] where len(it) > 0) -> I64
  noalloc {
  mut best := xs[0]
  for i in 1..len(xs) {
    if xs[i] > best {
      best = xs[i]
    }
  }
  best
}

fn shouting(s: String) -> Bool
  noalloc {
  byte_len(s) > 0 && s.ends_with("!") && !contains(s, "?")
}

fn areas(shapes: List[Shape]) -> Float
  noalloc {
  mut sum := 0.0
  for s in shapes {
    sum += s.area()
  }
  sum
}

fn main() {
  xs := [3.to_i64(), -4.to_i64(), 5.to_i64()]
  print(total(xs))
  print(weighted(xs))
  print(largest(xs))
  print(Point { x: 3.to_i64(), y: -4.to_i64() }.manhattan())
  print(shouting("go!"))
  print(areas([Circle { r: 1.0 }, Square { side: 2.0 }]))
}
