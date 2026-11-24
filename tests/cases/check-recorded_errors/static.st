type Item = { n: Int }
impl Item {
  fn new(n: Int) -> Item { Item { n } }
  fn size(self) -> Int { self.n }
}
trait Zero {
  fn zero() -> Self
}
impl Zero for Item {
  fn zero() -> Self { Item { n: 0 } }
}
fn a() -> Int { Item.new(1) }
fn b() -> Item { Item.new("x") }
fn c() -> Item { Item.make() }
fn d[T: Zero]() -> Int { T.zero() }
fn e(i: Item) -> Int { i.nope() }
fn f() -> String { Item.zero() }
fn main() { print(1) }
