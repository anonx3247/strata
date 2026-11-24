from std.json use Json, parse

type Many[T] = List[T]
  where len(it) >= 2

fn read[T: Json](text: String) -> Result[T, String] {
  T.from_json(parse(text)!)
}

fn pairs(text: String) -> Result[Many[Int], String] {
  read(text)
}

fn evens(text: String) -> Result[List[Int where it % 2 == 0], String] {
  read(text)
}

fn names(text: String) -> Result[Dict[String, String where it != ""], String] {
  read(text)
}

fn main() {
  print(pairs("[1, 2, 3]"))
  print(pairs("[1]"))
  print(evens("[2, 4]"))
  print(evens("[2, 3]"))
  print(names("{\"a\": \"x\", \"b\": \"\"}"))
  let m: Many[String] = ["a", "b"]
  print(len(m))
}
