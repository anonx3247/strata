fn total[I: FiniteIterator[Int]](it: I) -> Int {
  it.fold(0, (a, b) -> a + b)
}

fn main() {
  xs := [3, 1, 2]
  it := xs.iter()
  print(it.len())
  for x in it {
    print(x)
  }
  print(total(xs.iter()))
  print(total((0..5).iter()))
  print([x * 10 for x in xs.iter() where x > 1].to_list())
  print(Set { 5, 1, 3 }.iter().to_list())
  d := Dict { "b": 2, "a": 1 }
  for (k, v) in d.iter() {
    print(f"{k}={v}")
  }
  print(xs.map(x -> x + 1))
  print(len(xs))
  print(to_list(Set { 2, 1 }))
}
