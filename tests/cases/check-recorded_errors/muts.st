type Pt = { x: Int, y: Int }
fn bump(mut n: Int) { n += 1 }
fn show(n: Int) -> Int { n }
fn shift(mut p: Pt) { p.x = 3 }
fn a() {
  x := 1
  x = 2
  mut y := 1
  y = 3
  bump(y)
  bump(mut y)
  _ := show(mut y)
  bump(mut x)
  bump(3)
  q := mut y
  p := Pt { x: 1, y: 2 }
  p.x = 4
  shift(mut p)
  mut r := Pt { x: 1, y: 2 }
  shift(r)
  f := k -> { y = k }
  f(mut y)
  for i in [1, 2] { i = 3 }
  if true {
    y := 5
    y = 6
  }
  y = 7
}
fn b(n: Int, mut m: Int) {
  n = 1
  m = 2
  bump(mut n)
}
fn c(mut n: Int) {
  n := 1
  n = 2
}
fn main() { print(1) }
