// std.array's shape contracts at the call sites, proven or refuted by verify
from std.array use Array, Matrix, zeros, arange, matmul

/// An n×n matrix of zeros.
fn square(n: Natural) -> Matrix {
  zeros([n, n])
}

/// Three matrices multiplied: each product's shape follows from `matmul`'s `ensures`.
fn chain(a: Matrix, b: Matrix, c: Matrix) -> Matrix
  requires a.shape[1] == b.shape[0] && b.shape[1] == c.shape[0]
  ensures it.shape == [a.shape[0], c.shape[1]] {
  matmul(matmul(a, b), c)
}

/// A 2×3 matrix times its transpose's shape: 2×2.
fn gram() -> Matrix {
  a := arange(6).reshape([2, 3])
  matmul(a, zeros([3, 2]))
}

/// Shapes that don't fit: 2×3 times 2×3.
fn mismatch() -> Matrix {
  matmul(zeros([2, 3]), zeros([2, 3]))
}
