// Small values (tuples, records, `Option`, `Result`, variants of at most 4
// words) returned from functions: in registers, kept on the stack by the
// callers that only read them, boxed where they are stored.

type P = { x: Int, y: Int }

type Named = { name: String, n: Int }

type Shape = Circle { r: Int } | Rect { w: Int, h: Int } | Dot

type Counter = { n: Int }

trait Area {
  fn area(self) -> Int
}

impl Area for P {
  fn area(self) -> Int {
    self.x * self.y
  }
}

impl Counter {
  fn next(mut self) -> Option[Int] {
    if self.n >= 3 {
      None
    } else {
      self.n += 1
      Some(self.n)
    }
  }
}

fn divmod(a: Int, b: Int) -> (Int, Int) {
  (a / b, a % b)
}

fn half(n: Int) -> Option[Int] {
  if n % 2 == 0 { Some(n / 2) } else { None }
}

fn check(n: Int) -> Result[Int, String] {
  if n < 0 { Err("negative: " + to_string(n)) } else { Ok(n * 2) }
}

fn named(i: Int) -> Named {
  Named { name: "n" + to_string(i), n: i }
}

fn shape(i: Int) -> Shape {
  match i % 3 {
    0 => Circle { r: i },
    1 => Rect { w: i, h: i + 1 },
    _ => Dot,
  }
}

fn shape_area(s: Shape) -> Int {
  match s {
    Circle { r } => 3 * r * r,
    Rect { w, h } => w * h,
    Dot => 0,
  }
}

/// A result that is a value made before: its block given back as it is.
fn pick(a: Named, b: Named, first: Bool) -> Named {
  if first { a } else { b }
}

/// Some branches make a new value, others give one held elsewhere.
fn either(a: Named, i: Int) -> Named {
  if i % 2 == 0 { Named { name: a.name + "!", n: i } } else { a }
}

fn first_of[T](xs: List[T]) -> Option[T] {
  get(xs, 0)
}

fn swap[A, B](p: (A, B)) -> (B, A) {
  (p.1, p.0)
}

fn sum_halves(xs: List[Int]) -> Result[Int, String] {
  mut s := 0
  for x in xs {
    h := half(x) ?? 0
    v := check(h)!
    s += v
  }
  Ok(s)
}

fn early(n: Int) -> Option[(Int, Int)] {
  if n == 0 {
    return None
  }
  q := half(n)?
  Some(divmod(q, 3))
}

fn move_by(mut p: P, d: Int) {
  p.x += d
  p.y -= d
}

fn main() {
  // returned and taken apart, matched, read
  (q, r) := divmod(17, 5)
  print(f"{q} {r}")
  match half(10) {
    Some(h) => print(f"half {h}"),
    None => print("odd"),
  }
  print(divmod(9, 4).1)
  for k in [-3, 4] {
    match check(k) {
      Ok(v) => print(v),
      Err(e) => print(e),
    }
  }
  n := named(7)
  print(f"{n.name} {n.n}")
  // stored: in a list, a dict, a union, a trait value, a closure
  mut ps := []
  mut d := Dict[String, Named] {}
  for i in 0..5 {
    mut ps.push(divmod(i * 7, 3))
    mut d.insert(f"k{i}", named(i))
  }
  print(ps)
  print(d)
  let u: Int | (Int, Int) = divmod(8, 3)
  print(u)
  let a: Area = P { x: 3, y: 4 }
  print(a.area())
  got := half(6)
  f := (k: Int) -> k + (got ?? 0)
  print(f(1))
  kept := named(9)
  g := () -> kept.name
  print(g())
  // generic functions, sums, results given back as they were
  print(first_of([named(1), named(2)]))
  print(swap((1, "one")))
  mut total := 0
  for i in 0..9 {
    total += shape_area(shape(i))
  }
  print(total)
  print(pick(named(1), named(2), false))
  x := named(3)
  print(either(x, 2))
  print(either(x, 3))
  print(sum_halves([2, 4, 6]))
  print(sum_halves([2, -4]))
  print(early(0))
  print(early(14))
  print(early(3))
  // equality, a method giving an `Option`, `mut` parameters, fields
  print(divmod(7, 2) == (3, 1))
  print(half(4) == Some(2))
  mut c := Counter { n: 0 }
  while mut c.next() is Some {
    print(c.n)
  }
  mut p := P { x: 1, y: 2 }
  move_by(mut p, 5)
  p.x = divmod(20, 6).0
  print(p)
  mut s := shape(4)
  s = shape(5)
  print(s)
  // assigned a call's result: the old value's block reused when nothing
  // else holds it, else a new one; a value given back as it was
  mut m := named(1)
  held := m
  m = either(m, 2)
  print(f"{m.name} {held.name}")
  m = either(m, 4)
  m = either(m, 5)
  print(m)
  mut t := divmod(10, 3)
  for i in 0..3 {
    t = divmod(t.0 + i * 10, 3)
  }
  print(t)
}
