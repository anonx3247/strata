fn main() {
  (tx, rx) := channel(1)
  scope {
    spawn {
      tx.send(1)
      if len([1, 2]) > 5 {
        tx.close()
      }
    }
  }
  tx.close()
  for x in rx {
    print(x)
  }
}
