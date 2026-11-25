// Lists grown inside arenas and outside: pushed onto in place (their
// header kept), grown where they are at their arena's top, grown by a copy
// when other objects lie above them, shared with another list that must
// keep seeing its own elements, extended (`+=`), inserted into, held in a
// record's field; then copied out of the arena.

from std.memory use arena

type Bag = { name: String, xs: List[Int] }

type Item = Small(Int) | Named(String)

/// Squares: nothing else is made between two pushes, so the buffer stays
/// its arena's last object and grows where it is.
fn squares(n: Int) -> List[Int] {
  mut xs := []
  for i in 0..n {
    mut xs.push(i * i)
  }
  xs
}

/// A string made before each push lies above the buffer: it grows by a
/// copy.
fn labels(n: Int) -> List[String] {
  mut out := []
  for i in 0..n {
    mut out.push(f"l{i}")
  }
  out
}

/// `b` starts as `a`, so pushing onto it makes a new header; `a` keeps
/// its own four elements, the slice its two.
fn shared() -> (List[Int], List[Int], List[Int]) {
  a := [1, 2, 3, 4]
  mut b := a
  for i in 5..40 {
    mut b.push(i)
  }
  s := a[1..3]
  mut c := s
  mut c.push(99)
  (a, b, c + s)
}

/// `+=`, `insert` and a field's list.
fn mixed(n: Int) -> (List[Int], List[Int], Bag) {
  mut xs := [0]
  for i in 0..n {
    xs += [i, i]
  }
  mut ys := []
  for i in 0..n {
    mut ys.insert(0, i)
  }
  mut bag := Bag { name: "b", xs: [] }
  for i in 0..n {
    mut bag.xs.push(i + 1)
  }
  (xs, ys, bag)
}

/// Lists of lists and of a union, each inner list grown too.
fn nested(n: Int) -> List[List[Item]] {
  mut rows := []
  for r in 0..n {
    mut row := []
    for c in 0..r {
      if c % 2 == 0 {
        mut row.push(Small(c))
      } else {
        mut row.push(Named(f"n{c}"))
      }
    }
    mut rows.push(row)
  }
  rows
}

fn main() {
  xs := arena(() -> squares(100000))
  print(len(xs))
  print(xs[99999] + xs[0] + xs[4])
  print(squares(5))
  ls := arena(() -> labels(3000))
  print(len(ls))
  print(ls[2999])
  (a, b, c) := arena(() -> shared())
  print(a)
  print(len(b))
  print(b[3] + b[4] + b[34])
  print(c)
  print(shared().2)
  (m, ys, bag) := arena(() -> mixed(500))
  print(len(m))
  print(m[999] + m[1000])
  print(len(ys))
  print(ys[0] - ys[499])
  print(len(bag.xs))
  print(bag.xs[499])
  rows := arena(() -> nested(60))
  print(len(rows))
  print(len(rows[59]))
  print(rows[59][58])
  print(rows[3])
  print(nested(4))
}
