// Where scoped allocation from roots would put each value (compiler/roots):
// one allocation of each verdict, counted by `--memory-sites`.

type Big = { a: Int, b: Int, c: Int, d: Int, name: String }

/// The result is built where the caller puts it (its region).
fn make(n: Int) -> Big {
  Big { a: n, b: n, c: n, d: n, name: "made" }
}

/// What it stores into its `mut` parameter is the caller's: counted.
fn fill(mut acc: List[Big], n: Int) {
  mut acc.push(Big { a: n, b: 0, c: 0, d: 0, name: "lent" })
}

fn main() {
  mut total := 0
  for i in 0..3 {
    // the loop body's region, reset each time round, holds the list, the
    // record and the result of `make`, built where the caller puts it
    each := [Big { a: i, b: 0, c: 0, d: 0, name: "each" }, make(i)]
    total += len(each)
  }
  // replaced each time round a loop: its old values stay counted
  mut last := Big { a: 0, b: 0, c: 0, d: 0, name: "first" }
  for i in 0..4 {
    last = Big { a: i, b: 0, c: 0, d: 0, name: "last" }
  }
  // kept by a list of the function's body: its region
  mut keep := []
  for i in 0..5 {
    mut keep.push(Big { a: i, b: 0, c: 0, d: 0, name: "kept" })
  }
  mut acc := []
  fill(mut acc, 1)
  // a temporary of its statement
  t := len([Big { a: 9, b: 0, c: 0, d: 0, name: "temp" }])
  print(f"{total} {last.a} {len(keep)} {len(acc)} {t}")
}
