fn deep(n: Int) -> Int {
  if n == 0 {
    0
  } else {
    1 + deep(n - 1)
  }
}

fn main() {
  print(deep(100000000))
}
