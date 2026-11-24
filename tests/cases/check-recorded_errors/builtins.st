fn a() -> String { abs(3) }
fn b() -> Int { abs("x") }
fn c(xs: List[Int]) -> String { len(xs) }
fn d(xs: List[Int]) -> Int { push(xs, "s") }
fn e(s: String) -> Int { split(s, ",") }
fn f(xs: List[String]) -> String { join(xs, 3) }
fn g() -> Int { sqrt(2.0) }
fn h(xs: List[Int]) -> Int { xs.map(x -> x + 1) }
fn k(s: String) -> Bool { s.starts_with(1) }
fn m(xs: List[Int]) -> String { sum(xs) }
fn main() { print(1) }
