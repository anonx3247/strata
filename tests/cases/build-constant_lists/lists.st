type Color = Red | Green | Blue

type Box = { names: List[String] }

fn known(name: String) -> Bool {
  contains(["Int", "Float", "Bool"], name)
}

fn keep(xs: List[String]) -> Box {
  Box { names: xs }
}

fn main() {
  mut hits := 0
  for n in ["Int", "x", "Bool", "y"] {
    if known(n) {
      hits += 1
    }
  }
  mut ys := ["a", "b"]
  mut ys.push("c")
  zs := ["a", "b"]
  let none: List[String] = []
  same := zs + none
  more := zs + ["d"]
  b := keep(["k"])
  mut ints := [1, 2, 3]
  mut ints.insert(0, 9)
  print(f"{hits} {ys} {zs} {same} {more} {b.names} {ints} {[Red, Blue]} {drop(zs, 1)}")
}
