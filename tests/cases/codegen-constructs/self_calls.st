trait Named {
  fn name(self) -> String
  fn make(n: Int) -> Self
  fn twice(n: Int) -> List[Self] {
    [Self.make(n), Self.make(n + 1)]
  }
}

type Box = { n: Int }

impl Named for Box {
  fn name(self) -> String {
    f"box {self.n}"
  }
  fn make(n: Int) -> Self {
    Box { n }
  }
}

impl Named for Int {
  fn name(self) -> String {
    f"int {self}"
  }
  fn make(n: Int) -> Self {
    n * 10
  }
}

fn main() {
  print(Box.twice(1).map(b -> b.name()))
  print(Int.twice(2).map(i -> i.name()))
  print(3.name())
}
