fn work(n: Int) -> Int {
  if n == 3 {
    [1, 2][5]
  } else {
    n
  }
}

fn main() {
  r := scope {
    a := spawn work(1)
    b := spawn work(3)
    await a + await b
  }
  print(r)
}
