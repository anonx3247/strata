trait Many {
  fn copies(self) -> List[Self]
}

type A = { n: Int }

impl Many for A {
  fn copies(self) -> List[A] {
    [self, self]
  }
}

fn main() {
  let m: Many = A { n: 1 }
  print(len(m.copies()))
}
