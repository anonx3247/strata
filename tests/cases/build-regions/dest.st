// Destinations (`--regions`): a function makes its result in the region
// its caller gives it, a statement's, a loop body's or a function body's.

type P = { name: String, n: Int, tags: List[String] }

type Pair = { a: P, b: P }

/// More than 4 words: never given as words.
type Q = { p: P, s: String, a: Int, b: Int, c: Int }

/// A record, a list and strings made in the caller's destination.
fn make(i: Int, label: String) -> P {
  P { name: f"{label}{i}", n: i, tags: [label, f"t{i}"] }
}

/// Its result is another function's: the destination passes through.
fn twice(i: Int) -> Pair {
  Pair { a: make(i, "a"), b: make(i + 1, "b") }
}

fn weight(p: P) -> Int {
  p.n + byte_len(p.name) + len(p.tags)
}

/// Leaves early with a value made in the destination.
fn first_big(xs: List[Int], k: Int) -> Result[Q, String] {
  for x in xs {
    if x > k {
      return Ok(Q { p: make(x, "big"), s: f"q{x}", a: x, b: 1, c: 2 })
    }
  }
  Err(f"none over {k}")
}

fn checked(x: Int) -> Result[Q, String] {
  if x < 0 {
    Err(f"negative {x}")
  } else {
    Ok(Q { p: make(x, "ok"), s: f"q{x}", a: x, b: 1, c: 2 })
  }
}

fn uses_try(xs: List[Int], k: Int) -> Result[Int, String] {
  q := first_big(xs, k)!
  r := checked(k - 5)!
  Ok(weight(q.p) + byte_len(q.s) + q.a + q.b + q.c + weight(r.p))
}

/// A loop body's values, freed each pass, with `break` and `continue`.
fn loops(n: Int) -> Int {
  mut s := 0
  for i in 0..n {
    p := make(i, "l")
    if p.n % 3 == 0 {
      continue
    }
    q := twice(i)
    if q.b.n > 50 {
      break
    }
    s += weight(p) + weight(q.a)
  }
  mut j := 0
  while j < 10 {
    p := make(j, "w")
    s += weight(p)
    j += 1
  }
  s
}

/// A function body's values, kept in a list built by pushes.
fn gather(n: Int) -> Int {
  mut out := []
  for i in 0..n {
    mut out.push(make(i, "g"))
  }
  mut s := 0
  for p in out {
    s += weight(p)
  }
  s
}

/// The result built by pushes: the list is counted, its elements made in
/// the destination.
fn build(n: Int) -> List[P] {
  mut out := []
  for i in 0..n {
    mut out.push(make(i, "b"))
  }
  out
}

/// A value from a call, then changed in place: copied, since a region's
/// object is never held once.
fn changed(i: Int) -> Int {
  mut p := make(i, "c")
  p.n = p.n + 1
  mut p.tags.push("more")
  weight(p)
}

fn grow(mut p: P, s: String) {
  mut p.tags.push(s)
}

/// A value lent `mut` stays counted.
fn lent(i: Int) -> Int {
  mut p := make(i, "m")
  grow(mut p, "x")
  weight(p)
}

/// A recursion copying its own results: `[n] + up(n - 1)`.
fn up(n: Int) -> List[Int] {
  if n == 0 {
    []
  } else {
    [n] + up(n - 1)
  }
}

/// A recursion whose strings grow: `to_string(n) + down(n - 1)`.
fn down(n: Int) -> String {
  if n == 0 {
    ""
  } else {
    to_string(n % 10) + down(n - 1)
  }
}

/// A recursion holding a value made by a call across the recursive call.
fn deep(n: Int) -> Int {
  if n == 0 {
    0
  } else {
    p := make(n, "d")
    deep(n - 1) + weight(p)
  }
}

/// Records a recursion copies (`{ ..q }`): the levels below stay counted.
fn bump(n: Int) -> Q {
  if n == 0 {
    Q { p: make(0, "z"), s: "q", a: 0, b: 1, c: 2 }
  } else {
    q := bump(n - 1)
    Q { ..q, a: q.a + 1 }
  }
}

/// Holds a block's values across the recursive call.
fn drive(k: Int) -> Int {
  if k == 0 {
    0
  } else {
    q := bump(20)
    s := down(10)
    drive(k - 1) + q.a + byte_len(s)
  }
}

/// Starts tasks that read what it is given: a value given to it stays
/// counted.
fn in_tasks(rows: List[P]) -> Int {
  sums := scope {
    mut hs := []
    for t in 0..2 {
      mut hs.push(spawn task_work(rows, t))
    }
    await hs
  }
  sum(sums)
}

fn task_work(rows: List[P], k: Int) -> Int {
  mut s := 0
  for r in rows {
    s += weight(make(r.n + k, r.name))
  }
  s
}

fn main() {
  mut total := 0
  for i in 0..200 {
    total += weight(make(i, "s"))
    total += weight(twice(i).b)
    p := make(i, "v")
    total += weight(p) + len(p.tags)
  }
  total += match uses_try([1, 5, 9], 5) {
    Ok(w) => w,
    Err(e) => byte_len(e),
  }
  total += match uses_try([1, 2], 4) {
    Ok(w) => w,
    Err(e) => byte_len(e),
  }
  total += loops(100) + gather(50) + len(build(30)) + changed(4) + lent(5)
  total += len(up(300)) + byte_len(down(300)) + deep(300)
  total += drive(50) + in_tasks(build(5))
  rows := build(4)
  sums := scope {
    mut hs := []
    for t in 0..3 {
      mut hs.push(spawn task_work(rows, t))
    }
    await hs
  }
  names := build(3).map(q -> q.name + q.tags.join(""))
  print(f"{total} {sum(sums)} {names}")
}
