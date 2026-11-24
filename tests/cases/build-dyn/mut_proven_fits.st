fn grow(mut n: Positive) {
  n += 1
}

fn main() {
  mut p: Natural = 5
  grow(mut p)
  print(p)
}
