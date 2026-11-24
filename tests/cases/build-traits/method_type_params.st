trait Boxed {
  fn wrap[U](self, other: U) -> (Self, U)
}

type K = { k: Int }

impl Boxed for K {
  fn wrap[U](self, other: U) -> (Self, U) {
    (self, other)
  }
}

fn main() {
  print(K { k: 1 }.wrap("x"))
  print(K { k: 2 }.wrap(3.5))
}
