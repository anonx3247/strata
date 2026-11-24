type Pt = { x: Int, y: Int }
type Sh =
  | Sq(Int)
  | Rc(Int, Int)
fn f(a: Int, b: Int) -> Int { a + b }
fn g(x: Int) -> Int { f(f(x)) + zz }
fn h() -> Sh { Rc(f(1), yy, 3) }
fn m() -> Sh { Sq }
fn n() -> Pt { Pt(1, 2) }
fn o(f: Int) -> Int { f }
fn main() { print(g(1)) }
