fn first_of(xs: List[Int]) -> Option[Int] {
  at := 0
  x := xs.get(at)?
  Some(x + xs[at])
}

fn table() -> List[Int] {
  comptime {
    at := 1
    range := 2
    mut xs := [0, 0, 0]
    xs[at] = range
    for i in 0..2 {
      xs[i] += 1
    }
    xs
  }
}

fn pushes() -> List[Int] {
  push := 1
  len := 2
  take := 3
  mut xs := [len]
  mut xs.push(push)
  mut xs.push(take)
  _ := mut xs.pop()
  xs
}

fn main() {
  at := 3
  print(first_of([5, 6]) ?? at)
  print(table())
  print(pushes())
  print(f"{at} {[1, 2, 3][at - 1]}")
}

test "shadowed in a test" {
  at := 1
  xs := [1, 2]
  assert xs[at] == 2
}
