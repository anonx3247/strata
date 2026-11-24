type Pt = { x: Int, y: Int }
type Sh =
  | Sq(Int)
  | Rc { w: Int, h: Int }
  | Dot
fn a() -> Pt { Pt { x: 1, y: "two" } }
fn b(p: Pt) -> String { p.x }
fn c() -> Sh { Sq("s") }
fn d() -> Sh { Rc { w: true, h: 2 } }
fn e(s: Sh) -> Int { match s { Sq(n) => n, Rc { w, h } => w + h, Dot => "z" } }
fn f() -> Int { Dot }
fn g(p: Pt) -> Pt { Pt { ..p, x: "q" } }
fn h(p: Pt) -> Bool { p.z }
fn k(s: Sh) -> String { match s { Sq(n) => n, _ => "o" } }
fn m(p: Pt) -> Int { p == Dot }
fn main() { print(1) }
