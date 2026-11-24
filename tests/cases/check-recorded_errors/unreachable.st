type Sh =
  | Sq(Int)
  | Rc(Int, Int)
  | Dot
trait T {
  fn go(self) -> Int
}
fn go() -> Int { 1 }
fn a(s: Sh) -> Int { match s { Sq(_) => 1, Rc(_, _) => 2, Dot => 3, _ => 4 } }
fn b(s: Sh) -> Int { match s { _ => 1, Dot => 2 } }
fn c(s: Sh) -> Int { match s { Dot => 1, Dot => 2, _ => 3 } }
fn d(b: Bool) -> Int { match b { true => 1, false => 2, _ => 3 } }
fn e(o: Option[Int]) -> Int { match o { Some(n) => n, None => 0, x => 1 } }
fn f(n: Int) -> Int { match n { 1 => 1, k => 2, 3 => 3 } }
fn main() { print(1) }
