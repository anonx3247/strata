type Pos = Int where it > 0

type Range = { lo: Int, hi: Int } where it.lo <= it.hi

fn clamp(x: Int, lo: Int, hi: Int) -> Int
  requires lo <= hi
  ensures lo <= it && it <= hi {
  if x < lo {
    lo
  } else if x > hi {
    hi
  } else {
    x
  }
}

fn bad_abs(x: Int) -> Natural {
  if x < -5 {
    0 - x
  } else {
    x
  }
}

fn width(r: Range) -> Natural {
  r.hi - r.lo
}

fn bad_total(xs: List[Int]) -> Natural {
  mut t := 0
  for x in xs {
    t += x
  }
  t
}

fn first_pos(xs: List[Pos]) -> Option[Pos] {
  match xs {
    [] => None,
    [x, ..] => Some(x),
  }
}

fn divide_all(xs: List[Int], d: Int) -> List[Int] {
  xs.map(x -> x / d)
}

fn half(x: Float) -> Float
  ensures it <= x || x < 0.0 {
  x / 2.0
}

fn wrong(x: Float) -> Float
  ensures it > x {
  x * 2.0
}

fn checked(n: Int) -> Int
  requires n > 10 {
  n - 10
}

fn sum_to(n: Natural) -> Natural {
  mut i := 0
  mut s := 0
  while i < n {
    i += 1
    s += i
  }
  s
}

fn fact(n: Natural) -> Positive {
  if n == 0 {
    1
  } else {
    n * fact(n - 1)
  }
}

fn spin(n: Int) -> Int {
  mut k := n
  loop {
    if k == 1 {
      break
    }
    k = k - 1
  }
  k
}

fn collatz(n: Int) -> Int
  terminates {
  if n <= 1 {
    0
  } else if n % 2 == 0 {
    1 + collatz(n / 2)
  } else {
    1 + collatz(3 * n + 1)
  }
}

fn todo_later(n: Int) -> Int {
  n + ?rest
}

fn label(s: String, n: Pos) -> String
  ensures byte_len(it) > 0 {
  s + to_string(n)
}

fn main() {
  print(clamp(5, 0, 3))
}
