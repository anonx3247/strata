type Matrix[R: I64, C: I64] = { cells: List[Float] }

/// One row more is not the same type.
fn same[R: I64, C: I64](m: Matrix[R, C]) -> Matrix[R + 1, C] {
  m
}

/// Beyond linear: the product of two parameters.
fn flat[R: I64, C: I64](m: Matrix[R, C]) -> Matrix[R * C, 1] {
  Matrix { cells: m.cells }
}

fn main() {
  u8 := 3
  print(u8)
}
