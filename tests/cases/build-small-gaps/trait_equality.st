trait Area {
  fn area(self) -> Int
}

type Sq = { side: Int }

type Ci = { r: Int }

impl Area for Sq {
  fn area(self) -> Int {
    self.side * self.side
  }
}

impl Area for Ci {
  fn area(self) -> Int {
    3 * self.r * self.r
  }
}

impl Area for Int {
  fn area(self) -> Int {
    self
  }
}

fn main() {
  let a: Area = Sq { side: 2 }
  let b: Area = Sq { side: 2 }
  let c: Area = Ci { r: 2 }
  let d: Area = 4
  print(a == b)
  print(d == d)
  let xs: List[Area] = [a, c, d]
  print(contains(xs, b))
  let ys: List[Area] = [Sq { side: 2 }, Ci { r: 2 }, 4]
  print(xs == ys)
  for (i, x) in enumerate(xs) {
    print(f"{i}: {x.area()}")
  }
  for 0 in [0, 0] {
    print("zero")
  }
}
