// Updates of values something else still holds: each is copied first, once
// (the copy is then held by one thing and changed in place), so the number
// of copies doesn't grow with the size; but a value read out of a container
// and updated back in the same step is copied every time.

fn kept(n: Int) -> Int {
  mut d := Dict[Int, Int] {}
  for i in 0..n {
    d = insert(d, i, i)
  }
  // `old := d`: both hold it; the next insert copies it, once
  old := d
  for i in 0..n {
    d = insert(d, n + i, i)
  }
  len(d) + len(old)
}

fn looped(n: Int) -> Int {
  mut d := Dict[Int, Int] {}
  for i in 0..n {
    d = insert(d, i, i)
  }
  // the loop holds what it runs over: the first insert copies it
  for (k, v) in d {
    d = insert(d, k, v + 1)
  }
  len(d)
}

fn captured(n: Int) -> Int {
  mut xs: List[Int] = []
  mut d := Dict[Int, Int] { 0: 0 }
  // a closure holds what it captures
  size := () -> len(d)
  for i in 0..n {
    d = insert(d, i, i)
  }
  size() + len(d)
}

fn grown(d: Dict[Int, Int], k: Int) -> Dict[Int, Int] {
  // `d` is the caller's too (it reads it after the call)
  mut e := d
  e = insert(e, k, k)
  e
}

fn passed(n: Int) -> Int {
  mut d := Dict[Int, Int] {}
  for i in 0..n {
    d = insert(d, i, i)
  }
  e := grown(d, -1)
  len(d) + len(e)
}

fn nested_read(n: Int) -> Int {
  mut dicts := Dict[Int, Dict[Int, Int]] {}
  for i in 0..n {
    // the inner dict, read out with `get`, is the outer one's too
    dicts = insert(dicts, 0, insert(get(dicts, 0) ?? Dict[Int, Int] {}, i, i))
  }
  len(dicts)
}

fn all(n: Int) -> List[Int] {
  [kept(n), looped(n), captured(n), passed(n), nested_read(n)]
}

fn main() {
  print(all(100))
  print(all(400))
}
