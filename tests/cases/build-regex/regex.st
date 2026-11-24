type Sku = String
  where matches(it, "[A-Z]{3}-[0-9]{4}")

fn check(s: String) -> Bool {
  matches(s, "[a-z]+(_[a-z0-9]+)*")
}

fn main() {
  for s in ["abc", "abc_d1", "Abc", "", "a__b", "héllo"] {
    print(f"{s}: {check(s)}")
  }
  print(matches("héllo", "\w+"))
  print(matches("日本語", "\p{Han}+"))
  print(matches("x", "(a|b|x)"))
  print(matches("2024-01-31", "\d{4}-\d{2}-\d{2}"))
  print("ab".matches("a.?b"))
  let k: Sku = "ABC-1234"
  print(k)
  let s: String = "abc-1234"
  let bad: Sku = s
  print(bad)
}
