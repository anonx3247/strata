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

impl[T: Zero] Zero for List[T] {
  fn zero() -> Self {
    [T.zero(), T.zero()]
  }
}

impl[A: Zero, B: Zero] Zero for Dict[A, B] {
  fn zero() -> Self {
    dict([(A.zero(), B.zero())])
  }
}

fn zero_of[T: Zero]() -> T {
  T.zero()
}

fn nested[T: Zero]() -> List[List[T]] {
  zero_of()
}

fn a() -> List[Int] {
  zero_of()
}

fn b() -> List[List[String]] {
  zero_of()
}

fn c() -> Dict[String, List[Int]] {
  zero_of()
}

fn d() -> List[List[Int]] {
  nested()
}

fn main() {
  print(a())
  print(b())
  print(c())
  print(d())
}
