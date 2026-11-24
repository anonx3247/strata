trait Describe {
  fn describe(self) -> String
}

type Thing = { n: Int }

impl Describe for Thing {
  fn describe(self) -> String {
    f"{inner(self.n)}"
  }
}

impl Thing {
  fn twice(self) -> Int {
    self.n * 2
  }
}

fn inner(k: Int) -> Int {
  [1, 2].map(x -> half(x + k)).fold(0, (a, b) -> a + b)
}

fn half(n: Int) -> Int
  requires n % 2 == 0 {
  n / 2
}

fn main() {
  print(Thing { n: 2 }.twice())
  print(Thing { n: 1 }.describe())
}
