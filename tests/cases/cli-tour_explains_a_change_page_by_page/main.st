from a use area, perimeter, half, steps

fn square(s: Int) -> Int {
  area(s, s)
}

fn main() {
  print(to_string(square(3) + perimeter(1, 2) + half(4) + steps(1)))
}
