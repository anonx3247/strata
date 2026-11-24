type Pos = Int where it > 0
type Bad = Int where it + 1
type Name = String where it != "" && len(it.code_points()) <= 3
type Pt = { x: Int where it >= 0 = -1, y: Int where 1 < it = 2, n: Name = "abcd" }
type Sh =
  | Sq { side: Pos = 0 }
  | Dot
fn f(a: Int where it > 0) -> Int where it > 1
  requires 1
  requires it > 0
  ensures "s"
  ensures it > a
{ a + 1 }
fn g(a: Int where 3, b: Float) -> Pos
  requires b * 2.0
  ensures it
{ 1 }
fn h(mut n: Name) -> Int {
  mut p: Pos = 1
  p = 0
  p = -3
  mut q: Int where it < 10 || it == 20 = 1
  q = 20
  q = 15
  n = ""
  n = "ok"
  if q > 2 { p = 0 }
  q
}
trait Sz {
  fn size(self) -> Int requires 2 ensures 1.5
  law nonneg(x: Self): 3
  law ok(x: Self): x.size() >= 0
}
impl Sz for Pt {
  fn size(self) -> Int requires 7 ensures it % 2 { 1 }
}
test "t" (n: Int, xs: List[Int]) { assert true }
fn main() { print(1) }
