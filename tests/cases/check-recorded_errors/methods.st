type Pt = { x: Int, y: Int }
impl Pt {
  fn norm(self) -> Int { self.x + self.y }
  fn scale(self, k: Int) -> Pt { Pt { x: self.x * k, y: self.y * k } }
  fn bad(self) -> String { self.x }
}
fn a(p: Pt) -> String { p.norm() }
fn b(p: Pt) -> Pt { p.scale("2") }
fn c(p: Pt) -> Int { p.scale(2, 3).norm() }
fn e(p: Pt) -> Int { p.scale(2).x }
fn main() { print(1) }
