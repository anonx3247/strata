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

fn total(xs: List[Sq | Ci | Int]) -> Int {
  xs.map(x -> x.area()).fold(0, (a, b) -> a + b)
}

fn f(x: Positive | String) -> String {
  f"{x}"
}

fn main() {
  print(total([Sq { side: 2 }, Ci { r: 1 }, 5]))
  let s: Sq | Ci = Ci { r: 2 }
  print(s.area())
  print(f(3))
  print(f("a"))
  let n: Int = 0 - 1
  print(f(n))
}
