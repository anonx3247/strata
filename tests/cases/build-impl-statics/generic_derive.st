from std.json use Json, parse, stringify

type Pair[T] = { a: T, b: T }

derive Json for Pair

type Box = { p: Pair[Int] }

derive Json for Box

fn main() {
  print(stringify(
    Box { p: Pair { a: 1, b: 2 } }.to_json() !! std.json.Null,
  ))
  print(Box.from_json(
    parse("{\"p\": {\"a\": 3, \"b\": 4}}") !! std.json.Null,
  ))
}
