from std.json use Json, parse

type Port = Int
  where 1 <= it && it <= 65535

fn read[T: Json](text: String) -> Result[T, String] {
  T.from_json(parse(text)!)
}

fn ports(text: String) -> Result[List[Port], String] {
  read(text)
}

fn maybe(text: String) -> Result[Option[Positive], String] {
  read(text)
}

fn table(text: String) -> Result[Dict[String, Natural], String] {
  read(text)
}

fn valid[T](x: T) -> String {
  match T.check(x) {
    Ok(_) => "ok",
    Err(e) => e,
  }
}

fn main() {
  print(ports("[80, 443]"))
  print(ports("[80, 70000]"))
  print(maybe("null"))
  print(maybe("0"))
  print(table("{\"a\": 1, \"b\": -2}"))
  let p: Port = 8080
  print(valid(p))
  print(Port.check(0))
  print(Positive.check(3))
}
