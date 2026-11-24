from std.time use SystemClock, ms

fn fib(n: Int) -> Int {
  if n < 2 {
    n
  } else {
    fib(n - 1) + fib(n - 2)
  }
}

fn main(clock: SystemClock) {
  (tx, rx) := channel()
  // a real sleep while the main thread waits: not a deadlock
  scope {
    spawn {
      clock.sleep(ms(100))
      tx.send(1)
      tx.close()
    }
    tx.close()
    print(rx.recv() ?? 0)
  }
  // a long computation while the main thread waits: not a deadlock
  r := scope {
    h := spawn fib(27)
    await h
  }
  print(r)
}
