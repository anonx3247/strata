type State = { n: Int, name: String, errs: List[String], at: (Int, String) }

type P = { x: Int, y: Int }

fn bump(mut s: State) {
  s.n += 1
  mut s.errs.push("bumped")
}

// replaces the whole value moved in
fn reset(mut s: State) {
  s = State { n: 42, name: "reset", errs: ["r"], at: (4, "d") }
}

// another argument mentions the variable: not moved
fn set_n(mut s: State, n: Int) {
  s.n = n
}

// passes it on
fn twice(mut s: State) {
  bump(mut s)
  bump(mut s)
}

fn both(a: State, b: Int) -> String {
  f"{a.n} {b}"
}

fn main() {
  mut s := State { n: 0, name: "s", errs: [], at: (0, "a") }
  mut i := 0
  while i < 1000 {
    s.n += 1
    s.name = "s" + to_string(i)
    mut s.errs.push(to_string(i))
    i += 1
  }
  print(f"{s.n} {s.name} {len(s.errs)}")
  // a copy keeps its value
  t := s
  s.n = -1
  print(f"{t.n} {s.n}")
  // so does the value in a list
  xs := [s]
  s.name = "changed"
  print(f"{xs[0].name} {s.name}")
  // an operand that reads `s` before a later one assigns it
  print(both(s, {
    s.n = 7
    2
  }))
  // a loop over a field the body assigns
  mut k := 0
  for e in s.errs {
    if k < 3 {
      mut s.errs.push(e)
    }
    k += 1
  }
  print(f"{k} {len(s.errs)}")
  // a `mut` parameter
  bump(mut s)
  print(f"{s.n} {s.errs[len(s.errs) - 1]}")
  twice(mut s)
  set_n(mut s, s.n + 1)
  print(f"{s.n} {len(s.errs)}")
  u := s
  reset(mut s)
  print(f"{s.n} {s.name} {u.n} {u.name}")
  // a task holding `s` while it changes
  r := scope {
    h := spawn s.n * 10
    s.n = 100
    await h
  }
  print(f"{r} {s.n}")
  // a tuple field, and a tuple
  s.at = (1, "b")
  mut pair := (1, "one")
  pair.1 = "uno"
  print(f"{s.at.1} {pair.1}")
  // a constant record (every field a literal): never written in place
  mut c := P { x: 1, y: 2 }
  c.x = 10
  d := P { x: 1, y: 2 }
  print(f"{c.x} {d.x}")
  print("done")
}
