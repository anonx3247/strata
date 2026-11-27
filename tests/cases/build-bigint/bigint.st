fn factorial(n: Int) -> Int {
  if n <= 1 {
    1
  } else {
    n * factorial(n - 1)
  }
}

fn fib(n: Int) -> Int {
  mut a := 0
  mut b := 1
  for _ in 0..n {
    t := a + b
    a = b
    b = t
  }
  a
}

fn main() {
  print(factorial(25))
  print(factorial(30) / factorial(28))
  print(fib(100))
  print(fib(100) % 1000007)
  big := 123456789012345678901234567890
  print(big)
  print(-big)
  print(big * big)
  print(big / -7)
  print(big % -7)
  print(-big / 7)
  print(-big % 7)
  print(big == 123456789012345678901234567890)
  print(big > fib(100))
  print(big < -big)
  print(abs(-big))
  print(min(big, 3))
  print(max(-big, -3))
  print(float(big))
  let huge: Float = 1000000000000000000000000000000.0
  print(floor(huge))
  print(parse_int("99999999999999999999999"))
  print(parse_int("-4611686018427387905"))
  n := 4611686018427387903
  print(n + 1)
  print(n + 1 - 1 == n)
  print([big, 1, -big].sorted())
  print(Set { big, big * 1, 2 })
  print(Dict { big: "big" })
  print(f"{big:>40}")
  print(9223372036854775807 + 1)
  print(-9223372036854775808 - 1)
  print(abs(-4611686018427387904))
  match n + 1 {
    4611686018427387904 => print("matched big"),
    _ => print("no"),
  }
  print(2 * 3 + 4 - 5)
  print(len([1, 2, 3]) * 1000000000000000000000)
  // big integers are freed: this loop's thousands of them leave none live
  mut acc: Int = big
  mut i: Int = 0
  while i < 2000 {
    acc = (acc * big + i) % (big * big)
    i += 1
  }
  print(acc > 0)
  ys := [big * 2, big * 3]
  zs := ys + [big * 4]
  print(len(zs))
}
