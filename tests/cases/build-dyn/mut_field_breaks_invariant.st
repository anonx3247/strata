fn grow(mut n: Int) {
  n += 1
}

type R = { lo: Int, hi: Int }
  where it.lo <= it.hi

fn main() {
  mut r := R { lo: 1, hi: 2 }
  grow(mut r.lo)
  grow(mut r.lo)
  print(r)
}
