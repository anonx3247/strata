fn hold_and_wait(rx: Receiver[String], depth: Int) -> String {
  xs := [f"level {depth}", "held"]
  if depth == 0 {
    rx.recv() ?? "ended"
  } else {
    hold_and_wait(rx, depth - 1) + xs[0]
  }
}

fn answer() -> Int {
  6 * 7
}

fn inner(rx: Receiver[String]) -> String {
  race {
    hold_and_wait(rx, 2) as s => s,
    hold_and_wait(rx, 3) as t => t,
  }
}

fn main() {
  (tx, rx) := channel()
  (full, sink) := channel(1)
  r := race {
    hold_and_wait(rx, 5) as s => f"waited {s}",
    answer() as n => f"answer {n}",
  }
  print(r)
  mut wins := 0
  for i in 0..100 {
    w := race {
      {
        full.send(f"v{i}") except { () }
        full.send(f"w{i}") except { () }
        "sent"
      } as a => a,
      scope {
        h := spawn hold_and_wait(rx, 3)
        await h
      } as b => b,
      select {
        rx.recv() as x => x ?? "none",
      } as c => c,
      inner(rx) as d => d,
      [f"{i}", "won"] as e => e[1],
    }
    if w == "won" {
      wins += 1
    }
  }
  print(f"won {wins} of 100")
  _ := tx
  mut n := 0
  for x in sink {
    n += 1
  }
  print(f"{n} left in the channel")
}
