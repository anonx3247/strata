fn total(it: Iterator[Int]) -> Int {
  mut t := 0
  for x in it {
    t += x
  }
  t
}

fn main() {
  print(total([1, 2].iter()))
  print(total([1, 2, 3].iter().map(x -> x * 10)))
}
