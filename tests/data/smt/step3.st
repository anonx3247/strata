type Pos = Int where it > 0

type Bag = { items: List[Pos], name: String }

type Sorted = List[Int] where len(it) < 2 || it[0] <= it[1]

fn first(xs: List[Int]) -> Int
  requires len(xs) > 0
{
  xs[0]
}

fn unchecked_first(xs: List[Int]) -> Int {
  xs[0]
}

fn second_or(xs: List[Int], d: Int) -> Int {
  get(xs, 1) ?? d
}

fn total(xs: List[Pos]) -> Natural {
  sum(xs)
}

fn bad_total(xs: List[Int]) -> Natural {
  sum(xs)
}

fn positives(xs: List[Int]) -> List[Pos] {
  xs.filter(x -> x > 0)
}

fn doubled(xs: List[Pos]) -> List[Pos] {
  xs.map(x -> x * 2)
}

fn bad_shift(xs: List[Pos]) -> List[Pos] {
  xs.map(x -> x - 1)
}

fn all_pos(xs: List[Pos]) -> Bool
  ensures it
{
  xs.all(x -> x > 0)
}

fn some_big(xs: List[Int]) -> Bool
  ensures it ==> len(xs) > 0
{
  xs.any(x -> x > 100)
}

fn literal() -> List[Pos] {
  [1, 2, 3]
}

fn bad_literal() -> List[Pos] {
  xs := [1, 0, 3]
  xs
}

fn pushed(xs: List[Pos]) -> List[Pos]
  ensures len(it) == len(xs) + 1
{
  push(xs, 4)
}

fn joined(a: List[Pos], b: List[Pos]) -> List[Pos]
  ensures len(it) == len(a) + len(b)
{
  a + b
}

fn head_tail(xs: List[Int]) -> Int {
  match xs {
    [] => 0,
    [x, ..rest] => x + len(rest),
  }
}

fn pair(xs: List[Int]) -> Int {
  match xs {
    [a, b] => a / b,
    _ => 0,
  }
}

fn window(xs: List[Int], i: Int) -> List[Int]
  ensures len(it) <= len(xs)
{
  xs[i..i + 2]
}

fn taken(xs: List[Pos], n: Int) -> List[Pos]
  ensures len(it) <= len(xs)
{
  take(xs, n)
}

fn counting(n: Natural) -> List[Natural]
  ensures len(it) == n
{
  range(0, n)
}

fn bag_first(b: Bag) -> Pos
  requires len(b.items) > 0
{
  b.items[0]
}

fn bag_add(b: Bag, n: Pos) -> Bag {
  Bag { ..b, items: push(b.items, n) }
}

fn found(xs: List[Pos]) -> Pos {
  match xs.find(x -> x > 3) {
    Some(x) => x,
    None => 1,
  }
}

fn sorted_head(s: Sorted) -> Bool
  ensures it
{
  len(s) < 2 || s[0] <= s[1]
}

fn lens(s: String, t: String) -> Natural {
  byte_len(s + t)
}

fn starts(s: String) -> Bool
  ensures it ==> len(s.code_points()) >= 2
{
  starts_with(s, "ab")
}

fn digits(s: String) -> Int {
  parse_int(s) ?? 0
}

fn same_lists(xs: List[Int]) -> Bool
  ensures it
{
  xs == xs
}

fn copies(xs: List[Int]) -> Bool
  ensures it
{
  ys := xs
  ys == xs
}

fn shown(n: Int) -> String {
  to_string(n)
}
