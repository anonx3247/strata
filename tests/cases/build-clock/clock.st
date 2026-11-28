from std.time use SystemClock, Clock, ms, seconds, elapsed

fn never(rx: Receiver[Int]) -> Int {
  xs := ["held", "too"]
  (rx.recv() ?? 0) + len(xs)
}

fn work(n: Int) -> Int {
  n * 2
}

fn timeout(clock: SystemClock, rx: Receiver[Int]) -> Option[Int] {
  race {
    never(rx) as r => Some(r),
    clock.sleep(ms(30)) => None,
  }
}

fn main(clock: SystemClock) {
  (tx, rx) := channel()
  start := clock.now()
  print(timeout(clock, rx))
  r := race {
    work(21) as n => n,
    {
      clock.sleep(seconds(5))
      0
    } as z => z,
  }
  print(r)
  s := select {
    rx.recv() as x => "got",
    clock.after(ms(20)) => "timed out",
  }
  print(s)
  mut timeouts := 0
  for _ in 0..20 {
    if timeout(clock, rx) is None {
      timeouts += 1
    }
  }
  print(f"timeouts {timeouts}")
  took := elapsed(start, clock.now()).value
  print(took >= 650000000 && took < 4000000000)
  _ := tx
}
