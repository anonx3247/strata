type Counter = { n: Int, seen: List[Int] }

type Box = { c: Counter }

/// Proven: `c` is held once at every call (`count_all` passes a counter it
/// made), and `step` gives it back held once.
fn step(mut c: Counter, x: Int) {
  c.n += 1
  mut c.seen.push(x)
}

fn count_all(xs: List[Int]) -> Counter {
  mut c := Counter { n: 0, seen: [] }
  for x in xs {
    step(mut c, x)
  }
  c
}

/// Proven: a list built in a loop, then returned (a fresh result).
fn squares(n: Int) -> List[Int] {
  mut out: List[Int] = []
  for i in 0..n {
    mut out.push(i * i)
  }
  out
}

/// Not proven: `keep` holds the same counter.
fn shared(xs: List[Int]) -> (Counter, Counter) {
  mut c := count_all(xs)
  keep := c
  c.n += 100
  (c, keep)
}

/// Not proven: a closure captures the list.
fn captured(n: Int) -> List[Int] {
  mut out := squares(n)
  f := (i: Int) -> len(out) + i
  mut out.push(f(1))
  out
}

/// Not proven: the counter is stored in a box first.
fn stored(xs: List[Int]) -> (Box, Counter) {
  mut c := count_all(xs)
  b := Box { c }
  c.n = 0
  (b, c)
}

/// Not proven: one caller passes a counter that another variable holds, so
/// `bump` can't assume its parameter is held once.
fn bump(mut c: Counter) {
  c.n += 1
}

fn aliased(xs: List[Int]) -> (Counter, Counter) {
  mut c := count_all(xs)
  d := c
  bump(mut c)
  (c, d)
}

fn first(c: Counter) -> Counter {
  c
}

/// Not proven: `first` keeps its argument (it returns it).
fn kept(xs: List[Int]) -> (Counter, Counter) {
  mut c := count_all(xs)
  d := first(c)
  c.n = 7
  (c, d)
}

/// Tricky: reading a field out and keeping it shares the field, not the
/// record. The record's test is proven; the list's is not.
fn field_kept(xs: List[Int]) -> (Counter, List[Int]) {
  mut c := count_all(xs)
  seen := c.seen
  mut c.seen.push(99)
  (c, seen)
}

/// Tricky: a dict made empty is a shared constant until its first insert,
/// which copies it; from then on it is the dict's own.
fn histogram(xs: List[Int]) -> Dict[Int, Int] {
  mut d := Dict[Int, Int] {}
  mut d.insert(0, 0)
  for x in xs {
    mut d.insert(x % 3, (get(d, x % 3) ?? 0) + 1)
  }
  d
}

fn main() {
  xs := [1, 2, 3, 4, 5]
  c := count_all(xs)
  (s1, s2) := shared(xs)
  (b, st) := stored(xs)
  (k1, k2) := kept(xs)
  (fc, seen) := field_kept(xs)
  (a1, a2) := aliased(xs)
  print([c.n, len(squares(5)), len(captured(3)), s1.n, s2.n, b.c.n, st.n, k1.n, k2.n, len(fc.seen), len(seen), len(histogram(xs)), a1.n, a2.n])
}
