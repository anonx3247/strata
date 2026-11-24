trait Convert[T] {
  fn convert(self) -> T
}

type Celsius = { deg: Int }

impl Convert[String] for Celsius {
  fn convert(self) -> String {
    f"{self.deg}C"
  }
}

impl Convert[Int] for Int {
  fn convert(self) -> Int {
    self * 2
  }
}

fn twice[A: Convert[String]](xs: List[A]) -> List[String] {
  xs.map(x -> x.convert())
}

fn main() {
  print(Celsius { deg: 20 }.convert())
  print(twice([Celsius { deg: 1 }]))
  print(21.convert())
}
