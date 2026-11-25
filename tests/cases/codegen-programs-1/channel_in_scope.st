fn main() {
  let (tx, rx): (Sender[Int], Receiver[Int]) = channel()
  r := scope {
    a := spawn 1
    tx.close()
    mut got := []
    for v in rx {
      mut got.push(v)
    }
    got
  }
  print(r)
}
