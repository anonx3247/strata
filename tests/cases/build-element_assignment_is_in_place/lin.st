type Counts = { name: String, xs: List[Int] }

/// A list of `n` zeros, then a million updates: in place, so as fast for a
/// million elements as for a thousand.
fn updated(n: Int) -> Int
  requires n > 0
{
  mut xs := range(0, n).map(x -> 0)
  mut i := 0
  while i < 1000000 {
    xs[i % n] += 1
    i += 1
  }
  xs[0] + xs[n - 1]
}

/// The same through a field of a record and through rows of a grid.
fn nested(n: Int) -> Int
  requires n > 0
{
  mut c := Counts { name: "c", xs: range(0, n).map(x -> 0) }
  mut grid := range(0, n).map(r -> [0, 0])
  mut i := 0
  while i < 1000000 {
    c.xs[i % n] += 1
    grid[i % n][i % 2] += 1
    i += 1
  }
  c.xs[0] + grid[0][0] + grid[n - 1][1]
}

fn main() {
  print(updated(1000))
  print(updated(1000000))
  print(nested(1000))
  print(nested(1000000))
}
