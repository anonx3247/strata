trait Zero {
  fn zero() -> Self
}

impl Zero for Int {
  fn zero() -> Self {
    0
  }
}

impl Zero for String {
  fn zero() -> Self {
    "z"
  }
}

trait Pad {
  fn padded(self, n: Int) -> Self
}

impl[T: Zero] Pad for List[T] {
  fn padded(self, n: Int) -> Self {
    mut out := self
    while len(out) < n {
      mut out.push(T.zero())
    }
    out
  }
}

type Cell[T] = { v: Option[T] }

impl[T: Zero] Cell[T] {
  fn get_or_zero(self) -> T {
    match self.v {
      Some(x) => x,
      None => T.zero(),
    }
  }
}

fn pad_all[T: Zero](xs: List[List[T]], n: Int) -> List[List[T]] {
  xs.map(x -> x.padded(n))
}

fn ints() -> List[List[Int]] {
  pad_all([[1], []], 3)
}

fn main() {
  let e: List[String] = []
  print(e.padded(2))
  print([1].padded(3))
  print(Cell { v: None }.get_or_zero() + 1)
  let c: Cell[String] = Cell { v: None }
  print(c.get_or_zero())
  print(ints())
}
