type Sh =
  | Sq(Int)
  | Rc { w: Int, h: Int }
  | Dot
fn take(s: Sh) -> Int { 0 }
fn a(s: Sh) -> Int { match s { Sq(_) => s, _ => 0 } }
fn b(s: Sh) -> String { match s { Rc { .. } => s.w, _ => "z" } }
fn c(s: Sh) -> Int { match s { Sq(_) => take(s), _ => 0 } }
fn d(s: Sh) -> Bool { match s { Sq(_) => s == Dot, _ => true } }
fn e(s: Sh) -> Int { match s { Sq(_) | Dot => s, _ => 0 } }
fn f(s: Sh) -> Int { match s { Rc { .. } => s.z, _ => 0 } }
fn h(s: Sh) -> Int {
  if s is Sq { return s }
  0
}
fn main() { print(1) }
