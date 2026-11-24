type Sh =
  | Sq(Int)
  | Rc { w: Int, h: Int }
  | Dot
fn a(s: Sh) -> Int { match s { Sq(n) => n, Dot => 0 } }
fn b(o: Option[Int]) -> Int { match o { Some(n) => n } }
fn c(r: Result[Int, String]) -> Int { match r { Err(_) => 0 } }
fn d(x: Bool) -> Int { match x { true => 1 } }
fn e(s: Sh) -> Int { match s { Rc { w, h } => w, Sq(_) | Dot => 0 } }
fn g(o: Option[Sh]) -> Int { match o { None => 1 } }
fn h(s: Sh) -> Int { match s { Sq(n) => n, _ => 0 } }
fn mk() -> Option[Sh] { None }
fn m() -> Int { match mk() { None => 0 } }
fn main() { print(1) }
