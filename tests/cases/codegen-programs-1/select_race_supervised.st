from std.time use SystemClock, VirtualClock, Clock, seconds, ms

fn slow(c: VirtualClock, n: Int) -> Int {
  c.sleep(seconds(n))
  n
}

fn answer(questions: Receiver[Int]) -> Int {
  questions.recv() ?? 0
}

fn risky(n: Int) -> Int {
  100 / n
}

fn main(clock: VirtualClock) {
  (tx, rx) := channel(1)
  (done_tx, done_rx) := channel()
  r := scope {
    h := spawn {
      tx.send(41)
      tx.close()
      done_tx.close()
      7
    }
    done_tx.close()
    tx.close()
    mut total := 0
    mut going := true
    while going {
      select {
        rx.recv() as v => match v {
          Some(x) => {
            total += x
          },
          None => {
            going = false
          },
        },
        clock.after(seconds(5)) => {
          going = false
        },
      }
    }
    for _ in done_rx {
    }
    total + await h
  }
  print(r)
  (ask, questions) := channel()
  w := race {
    answer(questions) as v => Some(v),
    clock.sleep(seconds(30)) => None,
  }
  ask.close()
  print(w)
  print(clock.now().value)
  z := race {
    slow(clock, 3) as a => a,
    slow(clock, 1) as b => b * 10,
  }
  print(z)
  safe := scope {
    good := supervised spawn risky(5)
    bad := supervised spawn risky(0)
    (await good, await bad)
  }
  print(safe.0)
  match safe.1 {
    Ok(x) => print(x),
    Err(p) => print(p.kind + ": " + p.message),
  }
  hs := scope {
    tasks := [spawn 1, spawn 2, spawn 3]
    await tasks
  }
  print(hs)
}
