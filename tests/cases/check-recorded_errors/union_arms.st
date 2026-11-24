fn a(x: Int | String) -> String { if x is Int { x } else { x } }
fn b(x: Int | String) -> Bool {
  if x is String { return true }
  x
}
fn c(x: Int | String) -> Bool { match x { Int => x, String => false } }
fn main() { print(1) }
