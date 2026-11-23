fn count_up(n: Natural) -> Int {
  mut i := 0
  mut s := 0
  while i < n {
    s += 1
    i += 1
  }
  s
}

fn seen_while(n: Natural) -> Int
  ensures it == count_up(n) {
  count_up(n)
}

fn spin(n: Int) -> Int {
  mut i := n
  while i != 0 {
    i -= 2
  }
  i
}

fn uses_spin(n: Int) -> Int
  ensures it == spin(n) {
  spin(n)
}

fn first_loop(n: Int) -> Int {
  mut i := 0
  loop {
    i += 1
    if i > n {
      break
    }
  }
  i
}

fn uses_loop(n: Int) -> Int
  ensures it == first_loop(n) {
  first_loop(n)
}

fn marked(n: Natural) -> Int
  terminates {
  mut i := n
  loop {
    if i <= 0 {
      break
    }
    i -= 1
  }
  i
}

fn uses_marked(n: Natural) -> Int
  ensures it == marked(n) {
  marked(n)
}

fn sum_to(n: Natural) -> Natural {
  if n == 0 {
    0
  } else {
    n + sum_to(n - 1)
  }
}

fn total(xs: List[Natural]) -> Natural {
  match xs {
    [] => 0,
    [x, ..rest] => x + total(rest),
  }
}

fn apply_sum(xs: List[Natural]) -> List[Natural] {
  xs.map(sum_to)
}

fn uses_apply(xs: List[Natural]) -> Int
  ensures it == len(apply_sum(xs)) {
  len(xs)
}

fn halve(n: Int) -> Int {
  if n <= 1 {
    0
  } else {
    1 + halve(n / 2)
  }
}

fn apply_halve(xs: List[Int]) -> List[Int] {
  xs.map(halve)
}

fn uses_halve(xs: List[Int]) -> Int
  ensures it == len(apply_halve(xs)) {
  len(xs)
}

fn apply_total(xss: List[List[Natural]]) -> List[Natural] {
  xss.map(total)
}

fn uses_total(xss: List[List[Natural]]) -> Int
  ensures it == len(apply_total(xss)) {
  len(xss)
}

fn down(n: Int) -> Int {
  mut i := n
  mut k := 0
  while i > 0 {
    i -= 1
    k += 1
  }
  k
}

fn uses_down(n: Int) -> Bool
  ensures it == (down(n) >= 0) {
  true
}

fn sees_down() -> Bool
  ensures it == (down(2) == 2) {
  true
}

fn sees_count() -> Bool
  ensures it == (count_up(2) == 2) {
  true
}

fn sees_spin() -> Bool
  ensures it == (spin(4) == 0) {
  true
}

fn sees_loop() -> Bool
  ensures it == (first_loop(2) == 3) {
  true
}

fn sees_marked() -> Bool
  ensures it == (marked(3) == 0) {
  true
}

fn sees_halve(xs: List[Int]) -> Bool
  ensures it == (len(apply_halve(xs)) == len(xs)) {
  true
}

fn iter_sum(xs: List[Int]) -> Int {
  mut s := 0
  for x in xs.iter() {
    s += x
  }
  s
}

fn sees_iter(xs: List[Int]) -> Bool
  ensures it == (iter_sum([]) == 0) {
  true
}

fn twice(n: Natural) -> Natural {
  sum_to(n) + sum_to(n)
}

fn sees_twice() -> Bool
  ensures it == (twice(0) == 0) {
  true
}
