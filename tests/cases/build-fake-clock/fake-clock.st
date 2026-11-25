from std.time use SystemClock, Clock, Instant, Duration, nanos, ms, seconds,
  elapsed

/// A clock whose time never moves.
type FrozenClock = { at: Int }

impl Clock for FrozenClock {
  fn now(self) -> Instant {
    Instant(self.at)
  }

  fn sleep(self, d: Duration) {
    ()
  }
}

/// A clock running `speed` times faster than the system's.
type FastClock = { inner: SystemClock, speed: Int }

impl Clock for FastClock {
  fn now(self) -> Instant {
    Instant(self.inner.now().value * self.speed)
  }

  fn sleep(self, d: Duration) {
    self.inner.sleep(nanos(d.value / self.speed))
  }
}

/// Any clock: here the system's, through the `Clock` trait.
fn timer_of(clock: Clock, rx: Receiver[Int]) -> String {
  select {
    rx.recv() as x => "received",
    clock.after(ms(20)) => "timer",
  }
}

fn main(clock: SystemClock) {
  (tx, rx) := channel()
  _ := clock.now()
  // the frozen clock's 1 ms never passes, though 50 ms of real time do
  frozen := FrozenClock { at: 5 }
  r := scope {
    spawn {
      clock.sleep(ms(50))
      tx.send(1)
      tx.close()
    }
    select {
      rx.recv() as x => "received",
      frozen.after(ms(1)) => "frozen timer",
    }
  }
  print(r)
  // a timer of 0 is ready at once, in any clock's time
  print(select {
    rx.recv() as x => "received",
    frozen.after(nanos(0)) => "frozen timer at 0",
  })
  // ten seconds of a clock 1000 times faster are 10 ms of the system's
  fast := FastClock { inner: clock, speed: 1000 }
  before := clock.now()
  print(select {
    rx.recv() as x => "received",
    fast.after(seconds(10)) => "fast timer",
  })
  print(elapsed(before, clock.now()).value < 2000000000)
  print(timer_of(clock, rx))
  tx.close()
}
