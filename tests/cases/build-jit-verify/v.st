fn pieces(s: String) -> Positive {
  len(split(s, "a")) - 1
}

fn loops(n: Natural) -> Int {
  mut i := 0
  while i != n * 2 + 1 {
    i += 2
  }
  i
}

fn fine(xs: List[String]) -> Natural {
  len(join(xs, ",").code_points())
}
