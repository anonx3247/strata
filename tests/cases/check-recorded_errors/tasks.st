fn work() -> Int { 1 }
fn a() -> Int { await 3 }
fn b() -> String {
  scope {
    h := spawn work()
    await h
  }
}
fn c() -> Int {
  scope {
    h := spawn work()
    h
  }
}
fn main() { print(1) }
