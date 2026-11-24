type Sh =
  | Sq(Int)
  | Rc(Int, Int)
  | Dot
fn a(x: Int | String) -> Bool { match x { Int => true, other => other } }
fn b(s: Sh) -> Int { match s { Dot => 0, other => other } }
fn c(s: Sh) -> Int { match s { Dot => 0, Sq(_) => 1, other => other } }
fn d(o: Option[Int]) -> String { match o { None => "n", other => other } }
fn e(mut s: Sh) -> Int { match s { Sq(_) => {
  s = Dot
  s
}, _ => 0 } }
fn main() { print(1) }
