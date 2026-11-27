// A record a closure or a task captures goes on the heap, not the stack.
// On the stack (a count of 0) it crossed to each task as it was, without
// what it holds crossing in turn, so every task counted the same table
// at once ("dup of a freed object" in check mode); and a closure returned
// from the function read the dead frame.

type Table = {
  rows: Dict[String, List[Bool]],
  n: Int,
}

fn has(t: Table, k: String, i: Int) -> Bool {
  match get(t.rows, k) {
    Some(r) => get(r, i) ?? false,
    None => false,
  }
}

fn rows() -> Dict[String, List[Bool]] {
  mut d := Dict[String, List[Bool]] {}
  for i in 0..100 {
    mut d.insert(f"r{i}", [true, i % 2 == 0, false])
  }
  d
}

fn count(t: Table, id: Int) -> Int {
  mut n := 0
  for r in 0..3000 {
    if has(t, f"r{(r + id) % 100}", r % 3) {
      n += 1
    }
  }
  n
}

fn each(ks: List[Int], f: (Int) -> Int) -> Int {
  scope {
    mut hs := []
    for k in ks {
      mut hs.push(spawn f(k))
    }
    sum(await hs)
  }
}

fn adder(n: Int) -> (Int) -> Int {
  t := Table { rows: rows(), n }
  (y: Int) -> t.n + y + len(t.rows)
}

fn main() {
  mut a := 0
  mut b := 0
  for _ in 0..10 {
    // captured by a closure that tasks run
    t := Table { rows: rows(), n: 0 }
    a += each([0, 1, 2, 3, 4, 5, 6, 7], id -> count(t, id))
    // captured by tasks
    u := Table { rows: rows(), n: 0 }
    b += scope {
      mut hs := []
      for id in 0..8 {
        mut hs.push(spawn count(u, id))
      }
      sum(await hs)
    }
  }
  f := adder(5)
  print(f"{a} {b} {f(1)}")
}
