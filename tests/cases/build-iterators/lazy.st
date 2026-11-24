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

impl FiniteIterator[Int] for Count {
  fn len(self) -> Natural {
    if self.end > self.at {
      self.end - self.at
    } else {
      0
    }
  }
}

type Naturals = { at: Int }

impl Iterator[Int] for Naturals {
  fn next(mut self) -> Option[Int] {
    self.at += 1
    Some(self.at - 1)
  }
}

fn total(c: Count) -> Int {
  mut t := 0
  for i in c {
    t += i
  }
  t
}

fn size[I: FiniteIterator[Int]](it: I) -> Int {
  mut k := 0
  for _ in it {
    k += 1
  }
  k * 100 + it.len()
}

fn first_square_over(n: Int) -> Int {
  squares := Naturals { at: 0 }.map(x -> x * x)
  for s in squares {
    if s > n {
      return s
    }
  }
  0
}

fn main() {
  c := Count { at: 2, end: 5 }
  print(total(c))
  print(size(c))
  print(first_square_over(50))
  evens := Naturals { at: 0 }.filter(n -> n % 2 == 0).map(n -> f"<{n}>")
  for e in evens {
    if e == "<8>" {
      break
    }
    print(e)
  }
  print(Count { at: 0, end: 5 }
    .map(n -> n + 1)
    .filter(n -> n != 3)
    .fold(0, (a, b) -> a + b))
  xs := [1, 2, 3]
  print(xs.map(x -> x * 2))
  print(len(xs))
}
