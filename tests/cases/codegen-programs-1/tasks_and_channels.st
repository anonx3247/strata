fn main() {
  (tx, rx) := channel()
  results := scope {
    a := spawn {
      tx.send(1) except { () }
      tx.send(2) except { () }
      10
    }
    b := spawn 32
    mut got := []
    for v in rx {
      mut got.push(v)
    }
    (await a + await b, got)
  }
  print(results)
}
