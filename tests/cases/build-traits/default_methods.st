trait Shown {
  fn shown(self) -> String
  fn twice(self) -> String {
    self.shown() + self.shown()
  }
}

impl Shown for Bool {
  fn shown(self) -> String {
    if self {
      "yes"
    } else {
      "no"
    }
  }
}

impl Shown for String {
  fn shown(self) -> String {
    self
  }
}

fn all_shown[T: Shown](xs: List[T]) -> String {
  join(xs.map(x -> x.twice()), " ")
}

fn main() {
  print(all_shown([true, false]))
  print(all_shown(["a", "b"]))
}
