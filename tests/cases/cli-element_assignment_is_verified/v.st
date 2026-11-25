type Pos = Int where it > 0

/// Every element doubled, in place.
fn doubled(xs: List[Int]) -> List[Int]
  ensures len(it) == len(xs)
{
  mut ys := xs
  mut i := 0
  while i < len(ys) {
    ys[i] = ys[i] * 2
    i += 1
  }
  ys
}

/// The first element set to `v`; the rest as they were.
fn first_set(xs: List[Int], v: Int) -> List[Int]
  requires len(xs) > 0
  ensures it[0] == v && len(it) == len(xs)
{
  mut ys := xs
  ys[0] = v
  ys
}

/// Element `i` of a grid's row `r` set.
fn put(grid: List[List[Int]], r: Int, c: Int, v: Int) -> List[List[Int]]
  requires 0 <= r && r < len(grid) && 0 <= c && c < len(grid[r])
  ensures len(it) == len(grid)
{
  mut g := grid
  g[r][c] = v
  g
}

/// A list of positive numbers stays one.
fn raised(xs: List[Pos], i: Int) -> List[Pos]
  requires 0 <= i && i < len(xs)
{
  mut ys := xs
  ys[i] = ys[i] + 1
  ys
}

/// Off by one: the last index is `len(xs) - 1`.
fn last_set(xs: List[Int], v: Int) -> List[Int] {
  mut ys := xs
  ys[len(ys)] = v
  ys
}

fn main() {
  print(doubled([1, 2, 3]))
  print(first_set([1, 2], 9))
  print(put([[1, 2], [3, 4]], 1, 0, 7))
  print(raised([1, 2], 1))
}
