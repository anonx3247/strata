trait Source {
  fn take(self) -> String
}

type Line = { rx: Receiver[String], name: String }

impl Source for Line {
  fn take(self) -> String {
    held := [self.name, "held"]
    (self.rx.recv() ?? "ended") + held[1]
  }
}

type Fixed = { text: String }

impl Source for Fixed {
  fn take(self) -> String {
    self.text
  }
}

fn bottom(rx: Receiver[String]) -> String {
  xs := ["bottom", "held"]
  (rx.recv() ?? "ended") + xs[0]
}

fn level(rx: Receiver[String], depth: Int) -> String {
  xs := [f"level {depth}", "held"]
  if depth == 0 {
    bottom(rx)
  } else {
    level(rx, depth - 1) + xs[0]
  }
}

fn pure(n: Int) -> Int {
  if n < 2 {
    n
  } else {
    pure(n - 1) + pure(n - 2)
  }
}

fn apply(f: () -> String) -> String {
  xs := ["apply", "held"]
  f() + xs[0]
}

fn from(s: Source) -> String {
  xs := ["from", "held"]
  s.take() + xs[0]
}

fn by_bound[T: Source](s: T) -> String {
  xs := ["bound", "held"]
  s.take() + xs[0]
}

fn each(rx: Receiver[String]) -> String {
  parts := [1, 2, 3].map(i -> f"{i}" + (rx.recv() ?? "ended"))
  parts[0]
}

fn chosen(rx: Receiver[String], f: (Receiver[String]) -> String) -> String {
  xs := ["chosen", "held"]
  f(rx) + xs[0]
}

fn main() {
  (tx, rx) := channel(1)
  mut wins := 0
  for i in 0..50 {
    line := Line { rx, name: f"line {i}" }
    w := race {
      level(rx, 6) as a => a,
      apply(() -> level(rx, 2)) as b => b,
      from(line) as c => c,
      by_bound(line) as d => d,
      each(rx) as e => e,
      chosen(rx, bottom) as f => f,
      from(Fixed { text: f"fixed {pure(10)}" }) as x => x,
    }
    if w == "fixed 55from" {
      wins += 1
    }
  }
  print(f"won {wins} of 50")
  // the losers left their waits: a value sent now is still there
  tx.send("late") except { () }
  print(rx.recv() ?? "none")
}
