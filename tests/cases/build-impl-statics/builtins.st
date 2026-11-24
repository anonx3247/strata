from std.json use Json, parse, stringify

type P = { name: String, tags: List[String] }

derive Json for P

fn send[T: Json](x: T) -> String {
  stringify(x.to_json() !! std.json.Null)
}

fn read[T: Json](text: String) -> Result[T, String] {
  T.from_json(parse(text)!)
}

fn ints(text: String) -> Result[List[Int], String] {
  read(text)
}

fn people(text: String) -> Result[Dict[String, List[P]], String] {
  read(text)
}

fn main() {
  print(send([1, 2, 3]))
  print(send(dict([("a", [P { name: "x", tags: [] }])])))
  print(send(Some(2.5)))
  print(ints("[1, 2]"))
  print(ints("[1, \"x\"]"))
  print(
    people("{\"team\": [{\"name\": \"y\", \"tags\": [\"t\"]}]}"),
  )
  print(people("{\"team\": [{\"name\": 3, \"tags\": []}]}"))
}
