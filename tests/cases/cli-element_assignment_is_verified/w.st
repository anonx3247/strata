type Pos = Int where it > 0

/// The others are left as they were.
fn first_set(xs: List[Int], v: Int) -> List[Int]
  requires len(xs) > 1
  ensures it[1] == xs[1]
{
  mut ys := xs
  ys[0] = v
  ys
}

/// Zero isn't positive.
fn zeroed(xs: List[Pos], i: Int) -> List[Pos]
  requires 0 <= i && i < len(xs)
{
  mut ys: List[Pos] = xs
  ys[i] = 0
  ys
}

fn set_all(n: Int, v: Int) -> List[Int] {
  mut ys: List[Pos] = [1, 2, 3]
  ys[n] = v
  ys
}

fn main() {
  print(first_set([1, 2], 9))
  print(set_all(1, 5))
  print(set_all(1, -5))
}
