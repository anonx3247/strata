fn f(o: Option[Positive]) -> Int {
  o ?? 0
}

fn main() {
  print(f(Some(3)))
  let x: Option[Int] = Some(-2)
  print(f(x))
}
