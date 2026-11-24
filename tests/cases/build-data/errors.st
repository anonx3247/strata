type E =
  | NotFound
  | Bad { why: String }

fn g(n: Int) -> Result[Int, E] {
  if n == 0 {
    Err(NotFound)
  } else if n < 0 {
    Err(Bad { why: "neg" })
  } else {
    Ok(n)
  }
}

fn h(n: Int) -> Result[Int, E] {
  v := g(n)!
  Ok(v * 2)
}

fn main() {
  print(h(3))
  print(h(0))
  print(h(-1))
  print(h(0) == Err(NotFound))
  (x, y) := (5, "s")
  print(f"{x}{y}")
}
