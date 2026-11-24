fn risky(s: String, n: Int) -> String {
  repeat(s, n)
}

fn main() {
  r := scope {
    a := supervised spawn risky("ab", 0 - 1)
    b := supervised spawn risky("ab", 2)
    (await a, await b)
  }
  match r.0 {
    Ok(x) => print(x),
    Err(p) => print(p.kind),
  }
  print(r.1)
}
