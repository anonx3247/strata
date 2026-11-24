from std.utf8 use encode, decode
use std.unicode
fn a() -> String { encode("x") }
fn b() -> List[Int] { encode(1) }
fn c(bs: List[Int]) -> String { decode(bs) }
fn d() -> String { unicode.max_code() }
fn e() -> Int { unicode.next_code("a") }
fn main() { print(1) }
