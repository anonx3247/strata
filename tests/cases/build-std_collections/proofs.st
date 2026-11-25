/// Elements `i` and `j` exchanged: both indices are in range, so each
/// `set_at` is proven, and the length stays.
fn swap(xs: List[Int], i: Int, j: Int) -> List[Int]
  requires 0 <= i && i < len(xs) && 0 <= j && j < len(xs)
  ensures len(it) == len(xs)
{
  mut ys := xs
  a := ys[i]
  ys = set_at(ys, i, ys[j])
  ys = set_at(ys, j, a)
  ys
}

/// The order is -1, 0 or 1, and exact on integers.
fn order(a: Int, b: Int) -> Int
  ensures it == if a < b { -1 } else if a == b { 0 } else { 1 }
{
  compare(a, b)
}

/// Wrong on an empty list.
fn first_to_zero(xs: List[Int]) -> List[Int] {
  set_at(xs, 0, 0)
}
