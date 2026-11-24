from std.io use Console, recorder, printed

fn get(x: Int) -> Int {
  x * 10
}

fn main() {
  mut r := recorder([])
  mut r.print("hi")
  print(get(4))
  print(printed(r))
}
