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
        tx.send(n)
      }
      tx.close()
    }
    for _ in 0..3 {
      spawn {
        for n in rx {
          out.send(work(n))
        }
        out.close()
      }
    }
    tx.close()
    out.close()
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
        nums.send(i)
      }
      nums.close()
    }
    spawn {
      for w in ["a", "b", "c"] {
        words.send(w)
      }
      words.close()
    }
    nums.close()
    words.close()
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
    print(f"{got_nums} {got_words.sort()}")
  }
}

fn ticks() {
  r := scope {
    t := spawn slow(22)
    (tx, rx) := channel(1)
    mut ticks := 0
    loop {
      select {
        await t as v => {
          tx.close()
          break
        },
        tx.send(ticks) => {
          ticks += 1
          _ := rx.recv()
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
