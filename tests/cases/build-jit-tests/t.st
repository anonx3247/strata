fn half(n: Int) -> Int
  requires n % 2 == 0 {
  n / 2
}

fn twice(n: Int) -> Int {
  half(n) * 4
}

test "passes" {
  assert twice(4) == 8
}

test "fails" {
  print("before")
  print(twice(3))
}

test "asserts" {
  x := [1, 2]
  assert len(x) == 3
}
