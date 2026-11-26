// std.array's contracts at the call sites, proven or refuted by verify: shapes, and the
// magnitude bounds that keep float results finite
from std.array use Array, Matrix, zeros, matmul, dot_bound

/// An n×n matrix of zeros.
fn square(n: Natural) -> Matrix {
  zeros([n, n])
}

/// A product's shape follows from `matmul`'s `ensures`; its bounds from the caller's.
fn chain(a: Matrix, b: Matrix) -> Matrix
  requires a.shape[1] == b.shape[0] && a.shape[1] < 1 << 40
  requires a.within(dot_bound()) && b.within(dot_bound())
  ensures it.shape == [a.shape[0], b.shape[1]] {
  matmul(a, b)
}

/// The mean of an array of probabilities: elements in [0, 1] are within any larger bound.
fn mean_of(a: Array) -> Float
  requires a.size() > 0 && a.size() < 1 << 40 && a.within(1.0) {
  a.mean()
}

/// Shapes that don't fit: 2×3 times 2×3.
fn mismatch() -> Matrix {
  matmul(zeros([2, 3]), zeros([2, 3]))
}
