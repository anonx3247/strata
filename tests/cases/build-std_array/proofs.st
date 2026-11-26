// std.array's shape contracts at the call sites, proven or refuted by verify
from std.array use Array, Length, zeros, arange

/// An n×n matrix of zeros.
fn square(n: Length) -> Array
  ensures it.shape == [n, n] {
  zeros([n, n])
}

/// Three matrices multiplied: each product's shape follows from `matmul`'s `ensures`.
fn chain(a: Array, b: Array, c: Array) -> Array
  requires len(a.shape) == 2 && len(b.shape) == 2 && len(c.shape) == 2
  requires a.shape[1] == b.shape[0] && b.shape[1] == c.shape[0]
  ensures it.shape == [a.shape[0], c.shape[1]] {
  a.matmul(b).matmul(c)
}

/// A 2×3 matrix times a 3×2 one: 2×2.
fn gram() -> Array {
  zeros([2, 3]).matmul(zeros([3, 2]))
}

/// Shapes that don't fit: 2×3 times 2×3.
fn mismatch() -> Array {
  zeros([2, 3]).matmul(zeros([2, 3]))
}
