from std.memory use arena

fn slow(n: Int) -> Int {
  mut s := 0
  for i in 0..n {
    s += i % 7
  }
  s
}

fn main() {
  got := scope {
    (tx, rx) := channel(1)
    t := spawn slow(1000)
    tx.send(t)
    tx.close()
    arena(() -> {
      a := "item " + to_string(len([1, 2, 3]))
      mut done := 0
      for h in rx {
        done = await h
      }
      xs := [a, to_string(done)]
      xs
    })
  }
  other := arena(() -> {
    mut ys := []
    for i in 0..1000 {
      mut ys.push(f"overwrite {i}")
    }
    len(ys)
  })
  print(got)
  print(other)
}
