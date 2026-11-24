fn g(x: Int) -> Int { x }
fn a() { g(1) }
fn b(c: Bool) -> Int { if c { 1 } }
fn d(c: Bool) { if c { g(1) } }
fn e() -> Int {
  mut s := 0
  s = "x"
  while 1 { s += 1 }
  assert 2
  -true
  !3
  return "r"
}
fn h(c: Bool) -> Int { match c { true => 1, false => "f" } }
fn k(c: Bool) -> Int {
  let y: String = 3
  if c { [1] } else { "s" }
  0
}
fn l(n: Int) -> Int {
  mut i := 0
  while i < n
    invariant i {
    i += 1
  }
  i
}
fn main() { print(1) }
