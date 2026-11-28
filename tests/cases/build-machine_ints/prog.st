/// Fields of machine integers, packed in one word, and a string beside them.
type P = { a: u3, name: String, b: u5, c: s12, d: u1 }

/// An `R` by `C` matrix, row by row.
type Matrix[R: I64, C: I64] = { cells: List[Float] }

/// A row of `C` cells.
type Row[C: I64] = { cells: List[Float] }

/// `m` with `row` below it.
fn push_row[R: I64, C: I64](m: Matrix[R, C], row: Row[C]) -> Matrix[R + 1, C] {
  Matrix { cells: m.cells + row.cells }
}

/// Two rows more, through `push_row` twice.
fn push_two[R: I64, C: I64](m: Matrix[R, C], row: Row[C]) -> Matrix[R + 2, C] {
  push_row(push_row(m, row), row)
}

/// The sum, proven to fit.
fn add_small(a: u3, b: u3) -> u3
  requires a.to_int() + b.to_int() < 8 {
  a + b
}

fn main() {
  mut p := P { a: to_u3(5), name: "x", b: to_u5(17), c: to_s12(-100), d: to_u1(1) }
  print(p)
  p.c = to_s12(2047)
  p.b = p.b + to_u5(3)
  q := P { ..p, a: wrap_u3(15) }
  print(q)
  print(q == p)
  print(q.name + to_string(q.d.to_int()))
  print(add_small(to_u3(3), to_u3(4)))
  let a: Matrix[2, 3] = Matrix { cells: [1.0, 2.0, 3.0, 4.0, 5.0, 6.0] }
  let s: Matrix[4, 3] = push_two(a, Row { cells: [0.0, 0.0, 0.0] })
  print(len(s.cells))
  x := wrap_u3(len(s.cells))
  print(x + to_u3(4))
}
