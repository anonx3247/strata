// std.array's contracts at the call sites, proven or refuted by verify: shapes, and the
// magnitude bounds that keep float results finite
from std.array use Array, Length, zeros, dot_bound, sum_bound

/// An n×n matrix of zeros.
fn square(n: Length) -> Array
  ensures it.shape == [n, n] {
  zeros([n, n])
}

/// A product's shape follows from `matmul`'s `ensures`; its bounds from the caller's.
fn chain(a: Array, b: Array) -> Array
  requires len(a.shape) == 2 && len(b.shape) == 2
  requires a.shape[1] == b.shape[0] && a.shape[1] < 1 << 40
  requires a.within(dot_bound()) && b.within(dot_bound())
  ensures it.shape == [a.shape[0], b.shape[1]] {
  a.matmul(b)
}

/// A mean whose bounds the caller states: `mean`'s precondition is proven here, so the
/// call skips its entry checks.
fn mean_of(a: Array) -> Float
  requires a.size() > 0 && a.size() < 1 << 40 && a.within(sum_bound()) {
  a.mean()
}

/// Shapes that don't fit: 2×3 times 2×3.
fn mismatch() -> Array {
  zeros([2, 3]).matmul(zeros([2, 3]))
}
