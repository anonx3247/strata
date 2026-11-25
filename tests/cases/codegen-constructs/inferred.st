fn fill(mut out: List[(String, Int)], n: Int) {
  mut out.push((f"k{n}", n))
}

fn merged(v: List[(Int, Int)]) -> List[(Int, Int)] {
  mut out := []
  for (a, b) in v {
    match get(out, len(out) - 1) {
      Some((lo, hi)) => if a <= hi + 1 {
        out = take(out, len(out) - 1) + [(lo, if b > hi {
          b
        } else {
          hi
        })]
      } else {
        mut out.push((a, b))
      },
      None => {
        mut out.push((a, b))
      },
    }
  }
  out
}

fn main() {
  mut binds := []
  fill(mut binds, 1)
  fill(mut binds, 2)
  for (name, n) in binds {
    print(f"{name}={n}")
  }
  print(merged([(1, 2), (3, 5), (9, 9), (10, 12)]))
  twice := (xs: List[String]) -> xs.map(x -> x + x)
  both := twice(["a"]) + twice(["b"])
  print(twice(["a", "c"]).filter(x -> !contains(both, x)))
  mut seen := 0
  for x in [1, 2, 3] {
    match x {
      2 => {
        seen += 10
      },
      _ => (),
    }
  }
  print(seen)
}
