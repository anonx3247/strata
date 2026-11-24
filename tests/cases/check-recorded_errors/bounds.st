trait Area {
  fn area(self) -> Int
}
type Sq = { s: Int }
type Pt = { x: Int }
impl Area for Sq {
  fn area(self) -> Int { self.s * self.s }
}
type Box[T] = { item: T }
impl[T] Box[T] {
  fn get(self) -> T { self.item }
  fn bad(self) -> Int { self.item }
}
fn total[T: Area](xs: List[T]) -> Int { 0 }
fn a(q: Sq) -> String { q.area() }
fn b(p: Pt) -> Int { total([p]) }
fn c(b: Box[Int]) -> String { b.get() }
fn d[T: Area](x: T) -> String { x.area() }
fn main() { print(1) }
