type Sh =
  | Sq(Int)
  | Rc(Int, Int)
  | Dot
fn a(s: Sh) -> Int { s }
fn e(o: Option[Int]) -> String { o }
fn f(o: Option[Int]) -> String { match o { Some(n) => n, None => "z" } }
fn g(o: Option[Int]) -> String { o ?? 1 }
fn h(s: Sh) -> Bool { s == 3 }
fn main() { print(1) }
