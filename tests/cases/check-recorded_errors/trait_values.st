trait Shape {
  fn area(self) -> Int
}
trait Named: Shape {
  fn name(self) -> String
}
type Sq = { s: Int }
type Pt = { x: Int }
impl Shape for Sq {
  fn area(self) -> Int { self.s }
}
impl Named for Sq {
  fn name(self) -> String { "sq" }
}
fn a(xs: List[Shape]) -> String { (xs[0] ?? Sq { s: 1 }).area() }
fn b() -> List[Shape] { [Sq { s: 1 }, Pt { x: 2 }] }
fn c(s: Shape) -> String { s.area() }
fn d(s: Shape) -> Int { s }
fn e[T: Named](x: T) -> String { x.area() }
fn f(q: Sq) -> Shape { q }
fn main() { print(1) }
