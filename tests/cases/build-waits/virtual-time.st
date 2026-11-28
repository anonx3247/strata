from std.time use VirtualClock, seconds

fn answer(rx: Receiver[Int]) -> Int {
  rx.recv() ?? 0
}

fn main(clock: VirtualClock) {
  (tx, rx) := channel()
  // a timeout no task moves the time for
  r := race {
    answer(rx) as v => f"answer {v}",
    clock.sleep(seconds(30)) => "timed out",
  }
  print(r)
  print(clock.now().value / 1000000000)
  // sleeps end in the order of their deadlines
  scope {
    spawn {
      clock.sleep(seconds(3))
      print("three")
    }
    spawn {
      clock.sleep(seconds(1))
      print("one")
    }
    spawn {
      clock.sleep(seconds(2))
      print("two")
    }
  }
  print(clock.now().value / 1000000000)
  // an `after` arm
  s := select {
    rx.recv() as v => f"got {v ?? 0}",
    clock.after(seconds(10)) => "after 10 s",
  }
  print(s)
  print(clock.now().value / 1000000000)
  _ := tx
}
