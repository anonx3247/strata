fn down(n: Int, xs: List[String]) -> Int {
  ys := [f"{n}"]
  down(n + 1, xs) + len(ys)
}

fn main() {
  r := scope {
    h := supervised spawn down(0, [])
    await h
  }
  match r {
    Ok(v) => print(v),
    Err(p) => print(f"{p.kind}: {p.message}"),
  }
  print("still running")
}
