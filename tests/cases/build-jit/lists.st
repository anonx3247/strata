fn main() {
  p := "[a-z]" + "[0-9]"
  print(matches("a1", p))
  print(matches("a", p + "("))
}
