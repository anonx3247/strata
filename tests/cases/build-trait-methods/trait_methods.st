trait Named {
  fn name(self) -> String
}

trait Shape: Named {
  fn area(self) -> Int
  fn bigger(self, other: Self) -> Self
  fn scaled(self, k: Int) -> Self
  fn tag[U](self, extra: U) -> (String, U)
  fn grow(mut self, by: Int)
  fn count(self, mut seen: Int) -> Int
}

type Sq = { side: Int }

type Ci = { r: Int }

impl Named for Sq {
  fn name(self) -> String {
    "square"
  }
}

impl Named for Ci {
  fn name(self) -> String {
    "circle"
  }
}

impl Shape for Sq {
  fn area(self) -> Int {
    self.side * self.side
  }

  fn bigger(self, other: Self) -> Self {
    if other.side > self.side {
      other
    } else {
      self
    }
  }

  fn scaled(self, k: Int) -> Self {
    Sq { side: self.side * k }
  }

  fn tag[U](self, extra: U) -> (String, U) {
    (f"sq{self.side}", extra)
  }

  fn grow(mut self, by: Int) {
    self.side += by
  }

  fn count(self, mut seen: Int) -> Int {
    seen += 1
    seen
  }
}

impl Shape for Ci {
  fn area(self) -> Int {
    3 * self.r * self.r
  }

  fn bigger(self, other: Self) -> Self {
    if other.r > self.r {
      other
    } else {
      self
    }
  }

  fn scaled(self, k: Int) -> Self {
    Ci { r: self.r * k }
  }

  fn tag[U](self, extra: U) -> (String, U) {
    (f"ci{self.r}", extra)
  }

  fn grow(mut self, by: Int) {
    self.r += by
  }

  fn count(self, mut seen: Int) -> Int {
    seen += 10
    seen
  }
}

trait Convert[T] {
  fn convert(self) -> T
}

trait Pretty: Convert[String] {
  fn pretty(self) -> String
}

impl Convert[String] for Sq {
  fn convert(self) -> String {
    f"<{self.side}>"
  }
}

impl Pretty for Sq {
  fn pretty(self) -> String {
    "pretty square"
  }
}

fn make(big: Bool) -> Shape {
  if big {
    Sq { side: 10 }
  } else {
    Ci { r: 1 }
  }
}

fn main() {
  let shapes: List[Shape] = [Sq { side: 2 }, Ci { r: 1 }]
  for s in shapes {
    print(s.name())
    print(s.area())
    print(s.scaled(3).area())
    print(s.bigger(s.scaled(2)).area())
    print(s.tag(7))
    print(s.tag("x"))
    mut n := 0
    print(s.count(mut n))
    print(n)
  }
  mut g: Shape = Ci { r: 2 }
  mut g.grow(3)
  print(g.area())
  let p: Pretty = Sq { side: 4 }
  print(p.pretty())
  print(p.convert())
  print(make(true).area())
  print(make(false).name())
}
