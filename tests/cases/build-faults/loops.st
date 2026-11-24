fn g(n: Int) -> Int {
  mut i := 0
  while i < 10 {
    i += 1
    if i == n {
      break
    }
    if i % 2 == 0 {
      continue
    }
  }
  i
}

fn pick(b: Bool) -> String {
  match b {
    true => "yes",
    false => return "no",
  }
}

fn main() {
  print(g(4))
  print(g(40))
  print(pick(true) + pick(false))
  print(f"{1.5} {true} {"q"} {len("héllo".code_points())}")
}
