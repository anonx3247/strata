type Count = { at: Int, end: Int }

impl Iterator[Int] for Count {
  fn next(mut self) -> Option[Int] {
    if self.at >= self.end {
      return None
    }
    self.at += 1
    Some(self.at)
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

fn main() {
  c := Count { at: 0, end: 6 }
  for x in c {
    if x == 2 {
      continue
    }
    if x == 5 {
      break
    }
    for y in c.take(2) {
      print(f"{x} {y}")
    }
  }
  print(c.at)
  print(c.len())
  print(c.to_list())
  print(c.fold(0, (a, b) -> a + b))
  print(c[2])
  words := ["x", "yy", "zzz"]
  for (i, w) in enumerate(words) {
    print(f"{i}:{w}")
  }
}
