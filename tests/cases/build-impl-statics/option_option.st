from std.json use Json, stringify

fn send[T: Json](x: T) -> String {
  match x.to_json() {
    Ok(j) => stringify(j),
    Err(e) => f"error: {e}",
  }
}

fn main() {
  let a: Option[Option[Int]] = Some(None)
  print(send(a))
  let b: Option[Option[Int]] = Some(Some(2))
  print(send(b))
  let c: Option[Option[Int]] = None
  print(send(c))
}
