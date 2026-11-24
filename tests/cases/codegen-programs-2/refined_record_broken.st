type Range = { lo: Int, hi: Int } where it.lo <= it.hi

fn main() {
  mut r := Range { lo: 1, hi: 5 }
  r.lo = 3
  print(r)
  r.lo = 9
  print(r)
}
