fn main() {
  (tx, rx) := channel()
  got := scope {
    t := spawn rx.recv()
    sent := tx.send(1)
    (await t, sent)
  }
  print(got)
  print(tx.send(2))
}
