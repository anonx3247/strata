fn main() {
  mut n: Int = 0
  for i in range(0, 2000000) {
    n += i
  }
  print(n)
  print(len(range(0, 1500000)))
}
