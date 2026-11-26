/// A small value with a counted field, made where the caller can keep it on
/// its stack.
fn made(n: Int) -> Option[List[Int]] {
  Some([n, n + 1])
}

/// Takes a small value and a `mut` argument: the call holds its other
/// arguments while it runs.
fn grown(o: Option[List[Int]], mut log: List[Int]) -> Option[List[Int]] {
  match o {
    Some(xs) => {
      mut log.push(len(xs))
      Some(xs + [9])
    },
    None => None,
  }
}

/// `s` is a call's result kept on the stack, read by a call that holds it:
/// its fields are dropped once, when `s` ends.
fn step(n: Int, mut log: List[Int]) -> Option[List[Int]] {
  s := made(n)
  a := grown(s, mut log)
  b := grown(s, mut log)
  match (a, b) {
    (Some(x), Some(y)) => Some(x + y),
    _ => None,
  }
}

fn main() {
  mut log := []
  mut total := 0
  for i in 0..1000 {
    match step(i, mut log) {
      Some(xs) => {
        total += len(xs)
      },
      None => (),
    }
  }
  print(f"{total} {len(log)}")
  print(step(3, mut log))
}
