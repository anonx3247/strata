type Pos = Int where it > 0

fn sum_to(n: Natural) -> Natural {
  mut i := 0
  mut s := 0
  while i < n {
    i += 1
    s += i
  }
  s
}

fn count_down(n: Int) -> Natural {
  mut k := n
  while k > 0 {
    k -= 1
  }
  if k < 0 {
    0
  } else {
    k
  }
}

fn bad_loop(n: Natural) -> Pos {
  mut i := n
  while i > 0 {
    i -= 1
  }
  i
}

fn with_invariant(n: Natural) -> Natural {
  mut i := 0
  while i < n
  invariant i <= n
  {
    i += 1
  }
  n - i
}

fn wrong_invariant(n: Natural) -> Int {
  mut i := 0
  while i < n
  invariant i < n
  {
    i += 1
  }
  i
}

fn find_first(xs: List[Int], x: Int) -> Int {
  mut i := 0
  while i < len(xs) {
    if xs[i] == x {
      return i
    }
    i += 1
  }
  -1
}

fn total(xs: List[Pos]) -> Natural {
  mut s := 0
  for x in xs {
    s += x
  }
  s
}

fn first_big(xs: List[Int]) -> Int {
  mut found := 0
  for x in xs {
    if x > 10 {
      found = x
      break
    }
  }
  found
}

fn skip_odd(xs: List[Int]) -> Int {
  mut n := 0
  for x in xs {
    if x % 2 == 1 {
      continue
    }
    n += 1
  }
  n
}

fn forever(n: Int) -> Int {
  mut k := n
  loop {
    k += 1
    if k > 100 {
      break
    }
  }
  k
}

fn divide_all(xs: List[Int], d: Int) -> Int {
  mut s := 0
  for x in xs {
    s += x / d
  }
  s
}

fn ensures_loop(n: Natural) -> Natural
  ensures it >= n
{
  mut r := n
  mut i := 0
  while i < 3 {
    r += i
    i += 1
  }
  r
}
