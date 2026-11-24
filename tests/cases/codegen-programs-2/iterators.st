type Naturals = { n: Int }

impl Iterator[Int] for Naturals {
  fn next(mut self) -> Option[Int] {
    self.n += 1
    Some(self.n - 1)
  }
}

fn total[I: FiniteIterator[Int]](it: I) -> Int {
  it.fold(0, (a, x) -> a + x)
}

fn firsts[I: Iterator[Int]](it: I, n: Int) -> List[Int] {
  mut out := []
  for x in it.take(n) {
    out = push(out, x)
  }
  out
}

fn main() {
  xs := [5, 6, 7, 8, 9]
  print(xs.iter().drop(2).to_list())
  print(xs.iter().filter(x -> x % 2 == 0).len())
  print(total(xs.iter().map(x -> x * 2)))
  print(xs.iter().get(3))
  print(xs.iter().get(9))
  it := xs.iter()
  print(it[1])
  print(it[1..3].to_list())
  print(firsts(Naturals { n: 10 }, 4))
  print(Naturals { n: 0 }.filter(x -> x % 3 == 0).take(3).to_list())
  s := Set { 3, 1, 2 }
  for x in s.iter() {
    print(x)
  }
  d := Dict { "a": 1, "b": 2 }
  for (k, v) in d.iter() {
    print(f"{k}={v}")
  }
  print([x + 1 for x in xs.iter().take(2)])
  print(join(["a", "b", "c"].iter().map(s -> s + "!"), ", "))
  print(set(xs.iter().map(x -> x % 3)))
  print(dict(xs.iter().map(x -> (x, x * x)).take(2)))
}
