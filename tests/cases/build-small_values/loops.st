// Loops calling functions that give small values: none of them allocates
// (the one allocation is the printed line).

type Item = { id: Int, weight: Int }

type Cursor = { at: Int, end: Int }

impl Iterator[Int] for Cursor {
  fn next(mut self) -> Option[Int] {
    if self.at >= self.end {
      None
    } else {
      self.at += 1
      Some(self.at)
    }
  }
}

fn divmod(a: Int, b: Int) -> (Int, Int) {
  (a / b, a % b)
}

fn half(n: Int) -> Option[Int] {
  if n % 2 == 0 { Some(n / 2) } else { None }
}

fn checked(n: Int) -> Result[Int, String] {
  if n % 1000 == 999 { Err("bad") } else { Ok(n * 2) }
}

fn item(i: Int) -> Item {
  Item { id: i, weight: i % 7 }
}

fn halves(n: Int) -> Option[Int] {
  h := half(n)?
  Some(h + 1)
}

fn main() {
  mut s := 0
  for i in 0..100000 {
    (q, r) := divmod(i, 7)
    s += q + r
    match half(i) {
      Some(h) => { s += h },
      None => { s += 1 },
    }
    match checked(i) {
      Ok(v) => { s += v },
      Err(e) => { s += byte_len(e) },
    }
    it := item(i)
    s += it.weight
    s += item(i).id
    s += halves(i) ?? 0
  }
  mut c := Cursor { at: 0, end: 100000 }
  for k in c {
    s += k
  }
  print(s)
}
