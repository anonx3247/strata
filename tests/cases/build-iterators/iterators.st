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

type Count = { at: Int, end: Int }

impl Iterator[Int] for Count {
  fn next(mut self) -> Option[Int] {
    if self.at < self.end {
      self.at += 1
      Some(self.at - 1)
    } else {
      None
    }
  }
}

type Items[T] = { items: List[T], pos: Int }

impl[T] Iterator[T] for Items[T] {
  fn next(mut self) -> Option[T] {
    if self.pos < len(self.items) {
      x := self.items[self.pos]
      self.pos += 1
      Some(x)
    } else {
      None
    }
  }
}

type Padded[T] = { inner: Items[T], extra: Int }

impl[T: Zero] Iterator[T] for Padded[T] {
  fn next(mut self) -> Option[T] {
    match mut self.inner.next() {
      Some(x) => Some(x),
      None => if self.extra > 0 {
        self.extra -= 1
        Some(T.zero())
      } else {
        None
      },
    }
  }
}

fn collect[T, I: Iterator[T]](it: I) -> List[T] {
  mut out := []
  for x in it {
    mut out.push(x)
  }
  out
}

fn sum(it: Count) -> Int {
  mut total := 0
  for i in it {
    if i == 2 {
      continue
    }
    if i == 6 {
      break
    }
    inner := Count { at: 0, end: i }
    for j in inner {
      total += j
    }
  }
  total
}

fn main() {
  print(collect(Items { items: [1, 2, 3], pos: 0 }))
  print(collect(
    Padded { inner: Items { items: [4, 5], pos: 0 }, extra: 2 },
  ))
  print(collect(
    Padded { inner: Items { items: ["a"], pos: 0 }, extra: 1 },
  ))
  print(sum(Count { at: 0, end: 10 }))
  pairs := Items { items: [(1, "x"), (2, "y")], pos: 0 }
  for (i, s) in pairs {
    print(f"{i}{s}")
  }
}
