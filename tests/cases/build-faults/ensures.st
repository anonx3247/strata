fn f(x: Int) -> Int
  ensures it > x {
  x
}

fn main() {
  print(f(2))
}
