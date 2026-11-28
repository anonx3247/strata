fn pick(xs: List[Int], i: Int) -> Int {
  xs[i]
}

fn main() {
  (tx, rx) := channel()
  scope {
    spawn pick([1, 2], 5)
    for x in rx {
      print(x)
    }
  }
}
