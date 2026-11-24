type Box[T] = { item: T }
type Sh =
  | Sq(Int)
  | Rc { w: Int, h: Int }
trait Area {
  fn area(self) -> Int
  fn name(self) -> String
}
type Pt = { x: Int }
impl Area for Pt {
  fn area(self) -> Int { 1 }
  fn extra(self) -> Int { 2 }
}
impl Pt {
  fn dup(self) -> Int { 1 }
  fn dup(self) -> Int { 2 }
}
trait Bad: Pt {
  fn f(self) -> Int
}
fn a(x: Nope) -> Int { 1 }
fn b(x: Box[Int, Int]) -> Int { 1 }
fn c() -> Int { Ghost(1) }
fn d(s: Sh) -> Int { match s { Sq(a, b) => a, Rc { w } => w } }
fn e(s: Sh) -> Int { match s { Sq(n) | Rc { w: m, .. } => 1 } }
fn f() -> Self { 1 }
fn g() -> Int {
  break
  1
}
fn h[T: Ghosty](x: T) -> Int { 1 }
fn k(s: Sh) -> Int { match s { Zap => 1, _ => 2 } }
fn main() { print(1) }
