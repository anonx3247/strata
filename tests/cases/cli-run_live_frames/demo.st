from std.io use Terminal

/// A point.
type P = { x: Int, y: Int }

/// A point kept with a number.
type Box = { p: P, n: Int }

fn wrap(p: P) -> Box {
  Box { p: P { x: p.x, y: p.y }, n: 1 }
}

fn push1(mut xs: List[Int]) {
  mut xs.push(1)
}

fn check_small(x: Int) -> Int requires x < 100 {
  x
}

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
        ys := xs
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

fn main(mut term: Terminal) {
  run_loop(mut term)
}
