from std.io use Console

fn line_of(c: Console) -> String {
  mut d := c
  mut d.read_line() ?? "the end of the input"
}

fn main(console: Console) {
  a := scope {
    h := spawn line_of(console)
    await h
  }
  b := scope {
    h := spawn line_of(console)
    await h
  }
  c := scope {
    h := spawn line_of(console)
    await h
  }
  print(f"{a} | {b} | {c}")
}
