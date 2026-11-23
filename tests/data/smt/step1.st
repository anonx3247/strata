type Pos = Int where it > 0

type Small = Int where 0 <= it && it < 100

fn half(n: Int) -> Int
  requires n % 2 == 0
  ensures it * 2 == n
{
  n / 2
}

fn safe_div(a: Int, b: Pos) -> Int {
  a / b
}

fn unsafe_div(a: Int, b: Int) -> Int {
  a / b
}

fn clamp(x: Int) -> Small {
  if x < 0 {
    0
  } else if x > 99 {
    99
  } else {
    x
  }
}

fn wrong_clamp(x: Int) -> Small {
  if x < 0 {
    0
  } else {
    x
  }
}

fn twice_half(n: Int) -> Int
  requires n >= 0
{
  m := n * 2
  half(m)
}

fn bad_half(n: Int) -> Int {
  half(n)
}

fn abs(x: Int) -> Int
  ensures it >= 0
{
  if x < 0 {
    return -x
  }
  x
}

fn count(mut n: Int) -> Pos {
  n = n * n
  n + 1
}

fn checks(a: Int, b: Bool) -> Int {
  assert a == a
  let c: Small = 5
  if b && a > 3 {
    assert a > 2
  }
  c
}

fn masks(x: Int) -> Int
  requires x >= 0
  ensures it <= x
{
  x & 255
}

fn shifts(x: Int, k: Int) -> Int {
  x << k
}

fn greet(name: String) -> String
  ensures it != name
{
  "hi " + name
}

type Name = String where it != ""

type Even = Int where it % 2 == 0

fn bump(mut n: Natural)
  requires n < 1000
{
  n = n + 1
}

fn use_bump(k: Natural) -> Natural
  requires k < 10
{
  mut m: Natural = k
  bump(mut m)
  m
}

fn bad_assign(x: Int) -> Int {
  mut e: Even = 0
  e = x
  e
}

fn good_assign(x: Int) -> Int {
  mut e: Even = 0
  e = x * 2
  e
}

fn short(a: Int, b: Int) -> Bool {
  b != 0 && a / b > 1
}

fn shorter(a: Int, b: Int) -> Bool {
  b == 0 || a % b == 0
}

fn implied(a: Int, b: Int) -> Bool {
  b > 0 ==> a / b >= a / (b + 1)
}

fn named(s: String) -> Name {
  if s == "" {
    "anonymous"
  } else {
    s
  }
}

fn joined(a: Name, b: String) -> Name {
  a + b
}

fn formatted(n: Int) -> Name {
  f"n = {n / 1}"
}

fn floats(x: Float) -> Float {
  x + 1.0
}

fn lits() -> Int {
  (12 & 10) + (12 | 3) + (5 ^ 1) + (1 << 4) + (256 >> 2)
}

fn masked(x: Int) -> Int
  ensures 0 <= it && it < 16
{
  x & 15
}

fn bounded(x: Natural, y: Natural) -> Natural {
  x | y
}

fn neg_shift(x: Int) -> Int {
  x >> -1
}

fn chain(x: Int) -> Even {
  y := x * 4
  if y > 100 {
    return y
  }
  z := y + 2
  z
}

fn wrong_chain(x: Int) -> Even {
  if x > 3 {
    return x
  }
  0
}

fn twice(n: Even) -> Even {
  n + n
}

fn calls(a: Int) -> Even {
  twice(twice(a * 2))
}

fn bad_calls(a: Int) -> Even {
  twice(a)
}

fn strings(a: String, b: String) -> Bool
  ensures it == (a == b)
{
  a + "" == b + ""
}

fn unicode() -> String {
  "héllo \"quoted\" \\ end"
}

fn nested(a: Int, b: Int) -> Int
  requires b > 0
  ensures it >= 0
{
  if a > 0 {
    if a > b {
      a - b
    } else {
      b - a
    }
  } else {
    (a * a) % b
  }
}

fn eq_bools(p: Bool, q: Bool) -> Bool
  ensures it == (p == q)
{
  !(p != q)
}
