fn f(n: Int) -> Int {
  mut i := 0
  while i < n
    invariant i < 3 {
    i += 1
    if i == 1 {
      continue
    }
  }
  i
}

fn main() {
  print(f(2))
  print(f(5))
}
