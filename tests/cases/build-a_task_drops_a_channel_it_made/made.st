fn main() {
  scope {
    spawn {
      (tx, rx) := channel(1)
      tx.send(1) except { () }
      print("sent")
    }
  }
  print("done")
}
