fn p(s: String) -> Result[Int, String] { Ok(1) }
fn a(s: String) -> Result[String, String] { n := p(s)!
  Ok(n) }
fn b(s: String) -> String { p(s) except e { "x" } }
fn c(s: String) -> Int { p(s) except { "bad" } }
fn d(xs: List[Int]) -> String { xs[0] }
fn e(xs: List[Int]) -> Int { xs[1..2] }
fn f() -> String { 0..3 }
fn g(xs: List[Int]) -> Int { xs["a"] }
fn h(o: Option[Int]) -> Option[String] { n := o?
  Some(n) }
fn main() { print(1) }
