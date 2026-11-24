from std.time use VirtualClock, Clock, seconds, Instant, Duration

fn deep(c: VirtualClock, n: Int, held: String) -> String {
  if n == 0 {
    c.sleep(seconds(100))
    held + "!"
  } else {
    inner := held + to_string(n)
    deep(c, n - 1, inner)
  }
}

fn wait_on(clock: Clock) -> String {
  (tx, rx) := channel()
  tx.close()
  select {
    clock.after(seconds(1)) => "timer",
    rx.recv() as v => match v {
      Some(x) => x,
      None => "closed",
    },
  }
}

fn main(clock: VirtualClock) {
  w := race {
    deep(clock, 5, "start") as s => s,
    clock.sleep(seconds(1)) => "timeout",
  }
  print(w)
  (tx, rx) := channel()
  got := scope {
    spawn {
      for i in 0..3 {
        tx.send("item " + to_string(i))
      }
      tx.close()
    }
    tx.close()
    mut out := []
    for v in rx {
      out = push(out, v)
    }
    out
  }
  print(got)
  (out, inp) := channel(1)
  r := select {
    out.send("x" + "y") => "sent",
    clock.after(seconds(2)) => "late",
  }
  out.close()
  print(r)
  print(inp.recv())
  pair := scope {
    a := spawn "a" + "b"
    b := spawn 2
    await (a, b)
  }
  print(pair)
  print(wait_on(clock))
}
