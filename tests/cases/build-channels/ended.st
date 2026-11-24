fn main() {
  (tx, rx) := channel(1)
  tx.close()
  scope {
    spawn {
      tx.send(5)
      tx.close()
    }
  }
}
