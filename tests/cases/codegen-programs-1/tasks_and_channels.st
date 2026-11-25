fn main() {
  (tx, rx) := channel()
  results := scope {
    a := spawn {
      tx.send(1)
      tx.send(2)
      tx.close()
      10
    }
    b := spawn 32
    tx.close()
    mut got := []
    for v in rx {
      mut got.push(v)
    }
    (await a + await b, got)
  }
  print(results)
}
