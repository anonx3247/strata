type Pt = { x: Int, y: Int }
trait Area {
  fn area(self) -> Int
}
impl Area for Pt {
  fn area(self) -> Int { "a" }
}
impl Pt {
  fn me(self) -> Self { 3 }
}
fn twice(p: Pt, k: Int) -> Int { k }
fn a(p: Pt) -> String { p.area() }
fn b(p: Pt) -> Int { p.twice("x") }
fn main() { print(1) }
