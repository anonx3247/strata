fn run_loop(mut term: Terminal) {
  mut kept: List[Box] = []
  mut xs := [0]
  mut go := true
  mut t := 0
  while go {
    match mut term.read_line() {
      Some("tick") => {
        t += 1
        b := wrap(P { x: t, y: t * 10 })
        mut kept.push(b)
        ys := [0]
        push1(mut xs)
        mut term.print(
          join(kept.map(k -> to_string(k.p.x) + "/" + to_string(k.p.y)), ",")
            + f" ys={len(ys)} xs={len(xs)}",
        )
      },
      _ => {
        go = false
      },
    }
  }
}

