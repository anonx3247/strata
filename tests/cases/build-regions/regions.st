// Statement regions (`--regions`): values that die in the statement that
// makes them are made in its region and freed when it ends.

from std.memory use arena

type P = { name: String, n: Int, tags: List[String] }

type Row = { id: Int, label: String }

fn weight(p: P) -> Int {
  p.n + byte_len(p.name) + len(p.tags)
}

fn count(xs: List[Int], f: (Int) -> Bool) -> Int {
  mut k := 0
  for x in xs {
    if f(x) {
      k += 1
    }
  }
  k
}

fn width(pair: (String, Int)) -> Int {
  byte_len(pair.0) + pair.1
}

/// Leaves its statement early: `return`, `break` and `continue` reset the
/// regions they leave.
fn early(n: Int) -> Int {
  mut s := 0
  for i in 0..n {
    if byte_len(f"{i}") > 2 {
      break
    }
    if i % 2 == 0 {
      s += weight(P { name: f"e{i}", n: 1, tags: [] })
      continue
    }
    if i == 77 {
      return s + width((f"r{i}", i))
    }
    s += 1
  }
  s
}

/// What a task reads stays counted; each task's own statements have their
/// own regions.
fn task_work(rows: List[Row], k: Int) -> Int {
  mut s := 0
  for r in rows {
    s += weight(P { name: f"{r.label}{k}", n: r.id, tags: [r.label] })
  }
  s
}

/// Keeps what its arguments hold, but not the arguments themselves: a value
/// inside an argument lent to a function stays counted.
fn keep_parts(mut out: List[String], p: P, xs: List[String]) {
  mut out.push(p.name)
  mut out.push(xs[0])
}

/// A list's slice and a literal joined: the `+` shares the literal's
/// elements with the result, so they stay counted.
fn wrap(seq: List[P], n: Int) -> Result[List[P], String] {
  Ok(seq[..len(seq) - 1] + [P { name: f"w{n}", n, tags: [f"t{n}"] }])
}

fn in_arena(i: Int) -> List[String] {
  mut out := []
  for j in 0..3 {
    mut out.push(f"a{i}.{j}")
    _ := weight(P { name: f"t{j}", n: j, tags: [f"x{j}"] })
  }
  out
}

fn main() {
  mut total := 0
  base := "item"
  big := 1000000000000000000000
  mut kept := []
  for i in 0..300 {
    total += weight(P { name: f"{base}-{i}", n: i * big, tags: [base, f"t{i}"] })
    total += byte_len(chr(65 + i % 26) + "x") + byte_len(trim(f"  {i} "))
    total += count([1, 2, i], x -> x > i - 1 + big - big)
    total += width((base + "!", i))
    mut kept.push(f"k{i}")
    keep_parts(mut kept, P { name: f"p{i}", n: 0, tags: [] }, [f"x{i}"])
    total += { s := f"in{i}"; byte_len(s) + byte_len(f"{s}{s}") }
  }
  total += early(10) + early(100) + early(1000)
  rows := [Row { id: 1, label: "a" }, Row { id: 2, label: "b" }]
  sums := scope {
    mut hs := []
    for t in 0..4 {
      mut hs.push(spawn task_work(rows, t))
    }
    await hs
  }
  made := arena(() -> in_arena(7))
  wrapped := match wrap([P { name: "a", n: 1, tags: [] }, P { name: "b", n: 2, tags: [] }], 5) {
    Ok(ps) => ps.map(q -> q.name + q.tags.join("")),
    Err(e) => [e],
  }
  print(f"{total} {len(kept)} {sum(sums)} {made} {get(kept, 899) ?? ""} {wrapped}")
}
