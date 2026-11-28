type T = { n: Int }

fn stash(t: T, mut ys: List[Int]) {
  mut ys.push(t.n)
}

fn bump(mut t: T, mut ys: List[Int]) {
  t.n = t.n + 1
  mut ys.push(t.n)
}

fn made(k: Int) -> List[Int] {
  mut out: List[Int] = []
  for i in 0..k {
    mut out.push(i)
  }
  out
}

fn main() {
  t := T { n: 7 }
  mut a := made(3)
  b := a
  t.stash(mut a)
  print(b)
  print(a)
  mut u := T { n: 1 }
  mut c := made(2)
  d := c
  mut u.bump(mut c)
  print(d)
  print(c)
}
