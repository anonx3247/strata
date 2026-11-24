type Pt = { x: Int, y: Int }
fn f(a: Int) -> Int {
  b := a + c
  mut s := 0
  s += b
  if b > 0 { d } else { a + s }
}
fn g(xs: List[Int]) -> Int {
  ys := xs.map(x -> x + y)
  for z in xs { print(z + w) }
  zs := [u * 2 for u in xs where u > v]
  p := Pt { x: 1, y: r }
  print(f"{p.x} {nowhere}")
  match ys { [h, ..rest] => h + len(rest) + k, _ => q }
}
fn l(n: Int) -> Int {
  mut i := 0
  while i < n
    invariant i <= n + m {
    i += 1
  }
  i
}
fn main() { print(f(1) + g([1]) + nope(2) + l(1)) }
