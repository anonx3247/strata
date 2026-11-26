lemma double_le(a: Int, b: Int)
  requires a <= b
  ensures a + a <= b + b

fn sum_to(n: Int) -> Int
  requires n >= 0
  ensures it >= 0 {
  mut i := 0
  mut s := 0
  while i < n
    invariant i <= n
    invariant s >= 0 {
    s += i
    i += 1
  }
  assert s + s >= 0 + 0 by double_le(0, s)
  s
}

fn count(n: Int) -> Int
  requires n >= 0
  decreases n {
  if n == 0 {
    0
  } else {
    count(n - 1) + 1
  }
}
