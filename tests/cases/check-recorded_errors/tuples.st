fn a() -> (Int, String) { (1, 2) }
fn b(p: (Int, String)) -> Int { p.1 }
fn c(p: (Int, String)) -> String { (n, s) := p
  n }
fn d(xs: List[Int]) -> String {
  mut out := ""
  for x in xs { out = out + x }
  out
}
fn e(xs: List[(Int, String)]) -> Int {
  mut t := 0
  for (n, s) in xs { t = t + s }
  t
}
fn f(xs: List[String]) -> Int {
  mut t := 0
  for (i, s) in enumerate(xs) { t = t + s }
  t
}
fn g(p: (Int, Int)) -> Int { p.2 }
fn h() -> List[Int] { [x * 2 for x in ["a"]] }
fn main() { print(1) }
