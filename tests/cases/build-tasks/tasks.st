fn fib(n: Int) -> Int {
  if n < 2 {
    n
  } else {
    fib(n - 1) + fib(n - 2)
  }
}

/// Stops at the first square over 25, waiting for the tasks started so far.
fn first_big(xs: List[Int]) -> Option[Int] {
  scope {
    for x in xs {
      h := spawn x * x
      if await h > 25 {
        return Some(x)
      }
    }
    None
  }
}

fn nested(n: Int) -> Int {
  scope {
    a := spawn scope {
      b := spawn n + 1
      await b * 2
    }
    await a
  }
}

/// Every task holds the same list and the same strings, copies parts of
/// them and drops them: the counts change on all threads at once.
fn shared(words: List[String], rounds: Int) -> Int {
  scope {
    hs := [spawn {
      mut kept: List[String] = []
      for r in 0..rounds {
        ys := words
        kept += [ys[(r + k) % len(ys)]]
        if len(kept) > 8 {
          kept = kept[1..]
        }
      }
      len(kept) + k
    } for k in 0..8]
    (await hs).fold(0, (a, b) -> a + b)
  }
}

fn main() {
  xs := [1, 2, 3]
  (a, b) := scope {
    x := spawn fib(25)
    y := spawn len(xs) + fib(20)
    await (x, y)
  }
  print(f"{a} {b}")
  print(f"{scope {
  await [spawn fib(n) for n in 20..26]
}}")
  print(f"{first_big([1, 3, 5, 7])}")
  print(f"{nested(4)}")
  mut total := 0
  for i in 0..4 {
    total += scope {
      await (spawn i * i)
    }
  }
  print(f"{total}")
  words := ["alpha", "beta", "gamma"].map(w -> w + "!")
  print(f"{shared(words, 20000)} {words}")
  scope {
    t := spawn print("from a task")
    await t
    print("after it")
  }
}
