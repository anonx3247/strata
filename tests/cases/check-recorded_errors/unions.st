type Id = Int | String
type Small = Int where it < 10
fn a(n: Natural) -> String { n }
fn b(n: Natural) -> Int { n + 1 }
fn c(x: Int | String) -> Bool { x }
fn d() -> Id { 2.5 }
fn e() -> Id { 3 }
fn f(s: Small) -> String { s }
fn g(n: Int) -> Natural { n }
fn h() -> Natural { -1 }
fn k(x: Id) -> Int { x }
fn a2(x: Bool | String | Int) -> Float { x }
fn b2(c: Bool) -> Float { if c { "s" } else { 1 } }
fn d2() -> Float { let y: Id = 3
  y }
fn e2(c: Bool) -> Float { z := if c { "s" } else { 1 }
  z }
fn main() { print(1) }
