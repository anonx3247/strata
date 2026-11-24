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

fn spin(n: Int) -> Int {
  mut k := n
  while true {
    k += 1
  }
  k
}

fn ok_sum() -> Int {
  Count { at: 0, end: 4 }
    .filter(x -> x > 1)
    .map(x -> x * 2)
    .fold(0, (a, b) -> a + b)
}

fn bad_lambda() -> Int {
  Count { at: 0, end: 4 }.map(x -> spin(x)).fold(0, (a, b) -> a + b)
}

fn filtered_len() -> Int {
  Count { at: 0, end: 10 }.filter(x -> x % 3 == 0).len()
}

fn main() {
  print(ok_sum())
  print(filtered_len())
  odd_squares := [n * n for n in Naturals { at: 0 } where n % 2 == 1]
  for s in odd_squares {
    if s > 50 {
      break
    }
    print(s)
  }
  c := Count { at: 0, end: 4 }
  doubled := [x * 2 for x in c]
  print(doubled.len())
  print(doubled.to_list())
  print([(a, b) for a in c for b in ["x", "y"] where a > 1])
  print(Set { x % 2 for x in c })
  print(Dict { x: x * x for x in c })
  print([x + 1 for x in [1, 2, 3] where x != 2])
}
