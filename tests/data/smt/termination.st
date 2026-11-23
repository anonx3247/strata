fn count_down(n: Int) -> Int {
  mut i := n
  mut k := 0
  while i > 0 {
    i -= 1
    k += 1
  }
  k
}

fn search(xs: List[Int], x: Int) -> Bool {
  mut lo := 0
  mut hi := len(xs)
  while lo < hi {
    mid := lo + (hi - lo) / 2
    if xs[mid] == x {
      return true
    }
    lo = mid + 1
  }
  false
}

fn spin(n: Int) -> Int {
  mut i := n
  while i != 0 {
    i += 1
  }
  i
}

fn forever() -> Int {
  mut k := 0
  loop {
    k += 1
    if k > 10 {
      break
    }
  }
  k
}

fn fact(n: Int) -> Int
  requires n >= 0 {
  if n == 0 {
    1
  } else {
    n * fact(n - 1)
  }
}

fn up(n: Int) -> Int
  requires n <= 100 {
  if n >= 100 {
    n
  } else {
    up(n + 1)
  }
}

fn ack(m: Int, n: Int) -> Int
  requires m >= 0 && n >= 0 {
  if m == 0 {
    n + 1
  } else if n == 0 {
    ack(m - 1, 1)
  } else {
    ack(m - 1, ack(m, n - 1))
  }
}

fn sum(xs: List[Int]) -> Int {
  match xs {
    [] => 0,
    [x, ..rest] => x + sum(rest),
  }
}

type Tree =
  | Leaf
  | Node { left: Tree, value: Int, right: Tree }

fn size(t: Tree) -> Int {
  match t {
    Leaf => 0,
    Node { left, right, .. } => size(left) + 1 + size(right),
  }
}

fn even(n: Int) -> Bool {
  if n == 0 {
    true
  } else {
    odd(n - 1)
  }
}

fn odd(n: Int) -> Bool {
  if n == 0 {
    false
  } else {
    even(n - 1)
  }
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

fn uses_collatz(n: Int) -> Int {
  collatz(n) + 1
}

fn uses_spin(n: Int) -> Int {
  spin(n)
}

fn double(n: Int) -> Int {
  n * 2
}

fn half(n: Int) -> Int {
  n / 2
}

fn checked(n: Int) -> Int
  requires n > 0 {
  n
}

trait Shape {
  fn area(self) -> Int
  fn twice(self) -> Int {
    self.area() * 2
  }
}

type Square = { side: Int }

impl Shape for Square {
  fn area(self) -> Int {
    self.side * self.side
  }
}

fn total_area(s: Square) -> Int {
  s.area() + s.twice()
}
