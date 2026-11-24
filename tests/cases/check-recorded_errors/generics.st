type Box[T] = { item: T }
fn id[T](x: T) -> T { x }
fn first[T](xs: List[T], d: T) -> T { d }
fn a() -> String { id(1) }
fn b() -> Int { first([1, 2], "z") }
fn c[T](x: T) -> Int { x }
fn d() -> Box[Int] { Box { item: "s" } }
fn e(b: Box[String]) -> Int { b.item }
fn f() -> Option[Int] { Some("x") }
fn g() -> Result[Int, String] { Err(3) }
fn mk() -> Option[Int] { None }
fn h() -> Int { mk() ?? "d" }
fn k() -> Option[String] { Some(id(2)) }
fn m() -> Option[Int] { 3 }
fn main() { print(1) }
