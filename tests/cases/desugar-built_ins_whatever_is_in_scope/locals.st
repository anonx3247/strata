/// Locals named as the built-ins that `xs[i]`, `xs[i] = v`, `a..b`,
/// slices and comprehensions are made of.
fn shadowed(n: Int) -> List[Int] {
  at := 1
  set_at := 2
  range := 3
  len := 4
  map := 5
  filter := 6
  fold := 7
  slice := 8
  dict := 9
  set := 10
  mut xs := [0, 0, 0, 0, 0]
  xs[at] = set_at
  mut grid := [[0, 0], [0, 0]]
  grid[at][0] = range
  mut total := 0
  for i in 0..n {
    total += xs[i % 5]
  }
  for i in 0..=2 {
    total += i
  }
  ys := [x * 2 for x in xs where x > 0]
  zs := [x + y for x in [1, 2] for y in [10, 20]]
  d := Dict { "a": len }
  s := Set { map, filter }
  tail := xs[1..]
  mid := xs[1..3]
  [total, xs[at], grid[1][0], ys.len(), ys[0], zs.len(), zs[3], d.get("a") ?? 0, s.len(), tail.len(), mid[0], fold + slice + dict + set]
}

fn main() {
  print(shadowed(10))
}
