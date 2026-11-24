from std.io use Console, recorder

fn report(mut console: Console, n: Int) {
  for i in range(0, n) {
    mut console.print(f"line {i}")
  }
}

fn main(mut console: Console) {
  mut r := recorder([])
  report(mut r, 3)
  report(mut console, 2)
  print(r.out)
  print(r)
}
