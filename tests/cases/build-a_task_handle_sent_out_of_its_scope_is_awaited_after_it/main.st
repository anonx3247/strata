fn slow(n: Int) -> Int {
  mut s := 0
  for i in 0..n {
    s += i % 7
  }
  s
}

fn main() {
  (tx, rx) := channel(4)
  scope {
    t := spawn slow(1000)
    tx.send(t)
  }
  tx.close()
  mut total := 0
  for h in rx {
    total += await h
  }
  print(total)
}
