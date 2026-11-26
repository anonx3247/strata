type T = Leaf(Int) | Node(List[T])

type Row = { name: String, tags: List[String] }

/// A list grown by pushes, made in the caller's destination.
fn squares(n: Int) -> List[Int] {
  mut out := []
  for i in 0..n {
    mut out.push(i * i)
  }
  out
}

/// Strings pushed, a `+=` and a `+` of two lists.
fn words(n: Int) -> List[String] {
  mut out := ["w"]
  for i in 0..n {
    mut out.push(f"w{i}")
    if i % 3 == 0 {
      out += [f"x{i}", "y"]
    }
  }
  out + ["end"]
}

/// Two lists sharing a buffer, each grown after.
fn shared(n: Int) -> Int {
  mut xs := squares(n)
  ys := xs
  mut zs := ys
  mut xs.push(1)
  mut zs.push(2)
  mut zs.push(3)
  len(xs) * 100 + len(ys) * 10 + len(zs) + xs[n] + zs[n]
}

/// A parameter's counted list copied into a region list.
fn extended(given: List[String]) -> List[String] {
  mut out := given
  mut out.push("more")
  out
}

/// Early exits out of a loop building a list.
fn first_big(xs: List[Int], limit: Int) -> Result[List[Int], String] {
  mut seen := []
  for x in xs {
    if x < 0 {
      continue
    }
    if x > limit {
      return Err(f"{x} over {limit} after {len(seen)}")
    }
    mut seen.push(x)
    if len(seen) > 5 {
      break
    }
  }
  Ok(seen)
}

fn uses_first(xs: List[Int]) -> Result[Int, String] {
  got := first_big(xs, 50)!
  Ok(len(got))
}

/// A list per pass of a loop, and a dict holding lists.
fn grouped(n: Int) -> Int {
  mut d := Dict[Int, List[String]] {}
  mut total := 0
  for i in 0..n {
    mut row := []
    for j in 0..i % 4 {
      mut row.push(f"r{i}.{j}")
    }
    total += len(row)
    mut d.insert(i % 5, row)
  }
  for (_, v) in items(d) {
    total += len(v)
  }
  total
}

/// A recursion whose results are held by its own result.
fn rebuilt(t: T) -> T {
  match t {
    Leaf(n) => Leaf(n + 1),
    Node(ks) => {
      mut out := []
      for k in ks {
        mut out.push(rebuilt(k))
      }
      Node(out)
    },
  }
}

fn leaves(t: T) -> Int {
  match t {
    Leaf(n) => n,
    Node(ks) => sum(ks.map(k -> leaves(k))),
  }
}

fn tree(d: Int) -> T {
  if d == 0 {
    return Leaf(d)
  }
  Node([tree(d - 1), Leaf(d), tree(d - 1)])
}

fn rows(n: Int) -> List[Row] {
  mut out := []
  for i in 0..n {
    mut tags := []
    for k in 0..i % 3 {
      mut tags.push(f"t{k}")
    }
    mut out.push(Row { name: f"n{i}", tags })
  }
  out
}

fn task_rows(rs: List[Row], k: Int) -> Int {
  mut names := []
  for r in rs {
    mut names.push(r.name + to_string(k + len(r.tags)))
  }
  len(names) + byte_len(join(names, ""))
}

fn main() {
  mut total := 0
  for i in 0..30 {
    total += len(squares(i)) + len(words(i)) + shared(i + 1)
    total += len(extended(["a", "b"]))
  }
  total += match uses_first([1, -2, 3, 70]) {
    Ok(n) => n,
    Err(e) => byte_len(e),
  }
  total += match uses_first([1, 2, 3, 4, 5, 6, 7, 8]) {
    Ok(n) => n,
    Err(e) => byte_len(e),
  }
  total += grouped(40)
  total += leaves(rebuilt(tree(6)))
  big := squares(300000)
  total += len(big) + big[299999] % 1000
  rs := rows(6)
  sums := scope {
    mut hs := []
    for t in 0..3 {
      mut hs.push(spawn task_rows(rs, t))
    }
    await hs
  }
  print(f"{total} {sum(sums)} {words(4)} {rows(3)}")
}
