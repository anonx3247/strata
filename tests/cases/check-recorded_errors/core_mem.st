fn main() {
  p := raw_alloc(8)
  store_i64(p, 0, 1)
  print(load_i64(p, 0))
}

fn keep(p: Ptr, f: (Int) -> Ptr) -> List[Ptr] {
  [p]
}

// a `Ptr` written inside a body
fn hidden() -> Int {
  let q: Ptr = raw_alloc(8)
  g := (x: Ptr) -> 1
  0
}
