capability NightLamp {
  fn on() -> Int { 1 }
}
type Pt = { x: Int }
pub extern c fn cabs(x: I64) -> I64
fn work(n: Int) -> Int { n + 1 }
fn opt(n: Int) -> Option[Int] { Some(n) }
fn loose() -> Int {
  h := spawn work(1)
  await h
}
fn sel(tx: Sender[Int], rx: Receiver[Int]) -> Int {
  scope {
    h := spawn work(2)
    select {
      work(3) as y => 1,
      tx.send(1) as z => 2,
      await h as r => r,
    }
  }
}
fn empty() -> Int {
  select { }
}
fn none() -> Int {
  race { }
}
fn tasks() -> Int {
  scope {
    a := spawn { return 1 }
    b := spawn { opt(2)? }
    c := spawn { xs := [1, 2].map(x -> spawn work(x))
      0 }
    await a
  }
}
fn chans() -> Int {
  scope {
    (tx, rx) := channel(1)
    (ty, ry) := channel(1)
    h := spawn { tx.send(1) except { () }
      0 }
    g := spawn { ty.send(1) except { () }
      0 }
    ty.close()
    1
  }
}
fn racer() -> Int {
  race {
    work(1) as a => a,
    { return 2 } as b => b,
  }
}
fn lamp() -> Int {
  l := NightLamp
  1
}
test "asks" (n: Int, p: Pt, l: NightLamp) { assert true }
fn main() { print(1) }
