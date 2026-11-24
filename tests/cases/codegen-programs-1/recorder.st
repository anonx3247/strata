from std.io use recorder, printed

fn main() {
  r := recorder(["a"])
  print(printed(r))
}
