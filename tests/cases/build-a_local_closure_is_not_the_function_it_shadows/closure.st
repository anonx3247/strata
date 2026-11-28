fn pick(xs: List[Int]) -> List[Int] {
  [len(xs)]
}

fn made(k: Int) -> List[Int] {
  mut out: List[Int] = []
  for i in 0..k {
    mut out.push(i)
  }
  out
}

fn main() {
  pick := (xs: List[Int]) -> xs
  mut a := made(3)
  mut b := pick(a)
  mut b.push(7)
  print(a)
  print(b)
}
