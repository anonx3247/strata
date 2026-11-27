/// Top-level functions named as the built-ins behind the sugar: the sugar
/// still means the built-ins, a call by name means these.
fn at(_xs: List[Int], _i: Int) -> Int {
  -1
}

fn set_at(_xs: List[Int], _i: Int, _x: Int) -> List[Int] {
  []
}

fn range(_a: Int, _b: Int) -> List[Int] {
  []
}

fn len(_s: String) -> Int {
  -2
}

fn map(_x: Int) -> Int {
  -3
}

fn main() {
  mut xs := [1, 2, 3]
  xs[0] = 7
  mut grid := [[0, 0], [0, 0]]
  grid[1][0] = 5
  mut total := 0
  for i in 0..3 {
    total += xs[i]
  }
  ys := [x * 2 for x in xs]
  tail := xs[1..]
  print([total, xs[0], grid[1][0], ys[2], tail[0]])
  print([at(xs, 0), set_at(xs, 0, 1).len(), range(0, 5).len(), len("abc"), map(1)])
}
