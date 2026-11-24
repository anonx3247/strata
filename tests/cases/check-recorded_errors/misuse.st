fn r() -> Result[Int, String] { Ok(1) }
fn o() -> Option[Int] { None }
fn a() -> Int { 3 except { 0 } }
fn b() -> Int { o() except { 0 } }
fn c() -> Result[Int, String] { n := 3!
  Ok(n) }
fn d() -> Option[Int] { n := r()?
  Some(n) }
fn e() -> Int {
  for x in 5 { print(x) }
  0
}
fn f(n: Int) -> Int { n.size }
fn g(n: Int) -> Int { n(2) }
fn h(n: Int) -> String { f"{n:.2}" }
fn k(s: String) -> String { f"{s:+}" }
fn main() { print(1) }
