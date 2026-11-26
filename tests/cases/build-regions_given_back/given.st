// Values a statement makes that a call or an f-string gives back as they
// are: they outlive the statement through the variable, so they stay
// counted (`--regions`).

/// Gives back a string it was given.
fn either(a: String, b: String) -> String {
  if a == "" {
    b
  } else {
    a
  }
}

fn main() {
  mut kept := []
  for i in 0..3 {
    e := either("", f"e{i}")
    j := join([f"j{i}"], ",")
    s := to_string(f"s{i}")
    r := repeat(f"r{i}", 1)
    o := f"{f"o{i}"}"
    c := f"{chr(97 + i)}"
    mut kept.push(e)
    mut kept.push(j)
    mut kept.push(s)
    mut kept.push(r)
    mut kept.push(o)
    mut kept.push(c)
  }
  print(kept)
}
