fn a() {
  xs := [1, 2]
  xs[0] = 5
}

fn b() {
  mut xs := [1, 2]
  xs[0] = "five"
}

fn c() {
  mut s := "abc"
  s[0] = "x"
}

fn main() {
  a()
}
