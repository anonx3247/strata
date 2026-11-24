fn r() -> Result[Int, String] { Ok(1) }
fn o() -> Option[Int] { None }
fn a() -> Int { r()! }
fn b() -> Int { o()? }
fn c() -> Option[Int] { Some(r()!) }
fn d() -> Result[Int, Int] { Ok(r()!) }
fn main() { print(1) }
