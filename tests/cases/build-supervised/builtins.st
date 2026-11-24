fn deep(xs: List[String], n: Int, k: Int) -> Int {
  ys := [f"frame {n}", "held"]
  if n == 0 {
    boom(k, xs) + len(ys)
  } else {
    deep(xs + ys, n - 1, k) + len(ys)
  }
}

fn boom(k: Int, xs: List[String]) -> Int {
  zero := len(xs) - len(xs)
  match k % 8 {
    0 => len(xs) / zero,
    1 => len(xs) % zero,
    2 => 1 << zero - 1,
    3 => floor(sqrt(float(zero - 1))),
    4 => ord(xs[0]),
    5 => byte_len(chr(zero - 1)),
    6 => byte_at(xs[0], 100),
    _ => floor(float(len(xs)) / float(zero)),
  }
}

fn main() {
  mut n := 0
  for i in 0..50 {
    r := scope {
      h := supervised spawn deep([f"start {i}"], 5, i)
      await h
    }
    match r {
      Ok(v) => print(f"no panic: {v}"),
      Err(p) => {
        n += 1
        if i < 8 {
          print(f"{p.kind}: {p.message}")
        }
      },
    }
  }
  print(f"{n} of 50 panicked, the program goes on")
}
