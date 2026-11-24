fn main() {
  print(matches("abc123", "[a-z]+[0-9]+"))
  print(matches("abc", "[0-9]+"))
  p := "(" + "x"
  print(matches("x", p))
}
