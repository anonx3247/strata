from std.io use Console


fn line_of(c: Console) -> String {
  mut d := c
  mut d.read_line() ?? "the end of the input"
}

fn pick(xs: List[Int], i: Int) -> Int {
  xs[i]
}

fn reads_and_fails(c: Console) -> String {
  scope {
    r := spawn line_of(c)
    p := spawn pick([1], 3)
    f"{await r} {await p}"
  }
}

fn main(console: Console) {
  mut lost := 0
  for i in 0..20 {
    r := race {
      line_of(console) as line => f"read {line}",
      [f"{i}", "computed"] as xs => xs[1],
    }
    if r == "computed" {
      lost += 1
    }
  }
  print(f"the reader lost {lost} of 20 races")
  s := scope {
    h := supervised spawn reads_and_fails(console)
    match await h {
      Ok(v) => v,
      Err(p) => f"contained: {p.message}",
    }
  }
  print(s)
}
