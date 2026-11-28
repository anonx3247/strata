fn work(n: Int) -> Int {
  n * n
}

fn slow(n: Int) -> Int {
  if n < 2 {
    n
  } else {
    slow(n - 1) + slow(n - 2)
  }
}

fn pool() {
  (tx, rx) := channel(4)
  (out, results) := channel()
  scope {
    spawn {
      for n in 1..=10 {
        tx.send(n) except { () }
      }
    }
    for _ in 0..3 {
      spawn {
        for n in rx {
          out.send(work(n)) except { () }
        }
      }
    }
    mut total := 0
    for r in results {
      total += r
    }
    print(f"total {total}")
  }
}

fn merge() {
  (nums, nums_rx) := channel()
  (words, words_rx) := channel(2)
  scope {
    spawn {
      for i in 0..5 {
        nums.send(i) except { () }
      }
    }
    spawn {
      for w in ["a", "b", "c"] {
        words.send(w) except { () }
      }
    }
    mut got_nums := 0
    mut got_words: List[String] = []
    mut nums_open := true
    mut words_open := true
    while nums_open || words_open {
      select {
        nums_rx.recv() as n => match n {
          Some(x) => {
            got_nums += x
          },
          None => {
            nums_open = false
          },
        },
        words_rx.recv() as w => match w {
          Some(x) => {
            got_words += [x]
          },
          None => {
            words_open = false
          },
        },
      }
    }
    print(f"{got_nums} {got_words.sorted()}")
  }
}

/// `slow(22)` once `go` says so (the first tick has happened: `ticks` doesn't
/// depend on how fast `slow` runs)
fn gated(go: Receiver[Int]) -> Int {
  _ := go.recv()
  slow(22)
}

fn ticks() {
  r := scope {
    (go_tx, go_rx) := channel(1)
    t := spawn gated(go_rx)
    (tx, rx) := channel(1)
    mut ticks := 0
    loop {
      select {
        await t as v => {
          break
        },
        tx.send(ticks) => {
          ticks += 1
          _ := rx.recv()
          if ticks == 1 {
            go_tx.send(1) except { () }
          }
        },
      }
    }
    ticks > 0
  }
  print(f"ticked {r}")
}

fn main() {
  pool()
  merge()
  ticks()
}
