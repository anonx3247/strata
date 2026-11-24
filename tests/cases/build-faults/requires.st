fn half(n: Int) -> Int
  requires n % 2 == 0 {
  n / 2
}

fn main() {
  print(half(4))
  print(half(3))
}
