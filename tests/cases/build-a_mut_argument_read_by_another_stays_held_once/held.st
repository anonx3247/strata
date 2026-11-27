type C = { d: Dict[Int, Int], xs: List[Int] }

fn put(mut c: C, k: Int) {
  mut c.d.insert(k, k)
}

fn size(c: C) -> Int {
  len(c.d)
}

fn next(mut c: C) -> Int {
  len(c.d)
}

fn grow(mut c: C, o: C) -> Int {
  mut c.xs.push(1)
  len(o.xs)
}

fn same(c: C) -> C {
  c
}

fn part(c: C) -> List[Int] {
  c.xs
}

fn grow_by(mut c: C, l: List[Int]) -> Int {
  mut c.xs.push(1)
  len(l)
}

fn reset(mut c: C, l: List[Int]) -> Int {
  c = C { d: Dict[Int, Int] {}, xs: [] }
  len(l)
}

impl C {
  fn add(mut self, o: C) -> Int {
    mut self.xs.push(1)
    len(o.xs)
  }
}

fn main() {
  mut c := C { d: Dict[Int, Int] {}, xs: [] }
  for _ in 0..3000 {
    put(mut c, size(c))
  }
  for _ in 0..3000 {
    put(mut c, len(c.d))
  }
  for _ in 0..3000 {
    put(mut c, next(mut c))
  }
  print(len(c.d))
  mut a := C { d: Dict[Int, Int] {}, xs: [1] }
  print(mut a.add(a))
  print(grow(mut a, a))
  print(grow(mut a, same(a)))
  print(grow_by(mut a, part(a)))
  print(grow_by(mut a, a.xs))
  print(len(a.xs))
  print(reset(mut a, a.xs))
  print(len(a.xs))
}
