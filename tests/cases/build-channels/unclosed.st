fn main() {
  (tx, rx) := channel(1)
  scope {
    spawn {
      _ := tx.send(1)
    }
  }
  for x in rx {
    print(x)
  }
  _ := tx.send(2)
}
