fn pick(xs: List[Int], i: Int) -> Int {
  ys := [f"held {i}", "more"]
  if i > 3 {
    xs[i] + len(ys)
  } else {
    pick(xs, i + 1) + len(ys)
  }
}

fn half(n: Int) -> Int
  requires n % 2 == 0 {
  n / 2
}

fn shown(r: Result[Int, Panic]) -> String {
  match r {
    Ok(v) => f"ok {v}",
    Err(p) => f"panicked ({p.kind}): {p.message} at {p.stack}",
  }
}

fn main() {
  (a, b, c) := scope {
    x := supervised spawn pick([1, 2], 0)
    y := supervised spawn 40 + 2
    z := supervised spawn half(3)
    await (x, y, z)
  }
  print(shown(a))
  print(shown(b))
  print(shown(c))
  // a dead task's copy of a sender is closed: the receiver's loop ends
  (tx, rx) := channel(4)
  (r, got) := scope {
    h := supervised spawn {
      tx.send("one")
      tx.send(f"two {pick([1], 0)}")
      tx.close()
      0
    }
    tx.close()
    mut got := []
    for s in rx {
      got += [s]
    }
    (await h, got)
  }
  print(f"{shown(r)}; received {got}")
  // a panic in an ordinary task below a supervised one is the supervised one's
  d := scope {
    h := supervised spawn scope {
      t := spawn pick([1], 0)
      await t + 1
    }
    await h
  }
  print(shown(d))
  // many, all freed
  mut n := 0
  for i in 0..50 {
    r := scope {
      h := supervised spawn pick([i], 0)
      await h
    }
    if r is Err {
      n += 1
    }
  }
  print(f"{n} of 50 panicked")
  // handled where it is awaited: `except` on the `Result`
  e := scope {
    h := supervised spawn half(5)
    await h except p {
      print(f"fell back after: {p.message}")
      0
    }
  }
  print(e)
  // a supervised race arm: its panic is its result, and the race goes on
  (hold, wait) := channel()
  w := race {
    supervised pick([1], 0) as r => match r {
      Ok(v) => f"won with {v}",
      Err(p) => f"lost: {p.kind}",
    },
    wait.recv() ?? "never" as x => x,
  }
  hold.close()
  print(w)
}
