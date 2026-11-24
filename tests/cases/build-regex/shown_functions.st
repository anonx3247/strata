fn twice(n: Int) -> Int {
  n * 2
}

type H = { name: String, on: (Int) -> Int }

fn main() {
  k := 3
  f := (x: Int) -> x + k
  print(f)
  print(H { name: "h", on: twice })
  print([f, (a: Int) -> a])
  print(f(1))
}
