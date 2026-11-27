fn main() {
  scope {
    spawn {
      (tx, rx) := channel(1)
      tx.send(1)
      if len([1, 2]) > 5 {
        tx.close()
      }
      print("sent")
    }
  }
  print("done")
}
