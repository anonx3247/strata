fn work() -> Int { 1 }
fn a() -> Int {
  mut keep := [spawn work()]
  scope {
    h := spawn work()
    keep = [h]
    await h
  }
}
fn b() -> Int {
  scope {
    h := spawn work()
    f := () -> await h
    f()
  }
}
fn c() -> Int {
  scope {
    h := spawn work()
    g := spawn (await h)
    await g
  }
}
fn main() { print(1) }
