// A big-data workload: 100,000 records as JSON text, parsed back and
// summed. Most of its memory is the document: the values, then the text,
// then the parsed tree.

from std.json use Json, JsonValue, Array, parse, stringify

type Item = { id: Int, name: String, tags: List[String], price: Int }

derive Json for Item

fn main() {
  mut items := []
  for i in 0..100000 {
    mut items.push(Item { id: i, name: f"item {i}", tags: [f"t{i % 13}", "sale"], price: i % 997 })
  }
  mut arr := []
  for it in items {
    match it.to_json() {
      Ok(j) => mut arr.push(j),
      Err(_) => (),
    }
  }
  text := stringify(Array(arr))
  print(f"{byte_len(text)} bytes")
  match parse(text) {
    Ok(Array(xs)) => {
      mut total := 0
      for x in xs {
        match Item.from_json(x) {
          Ok(it) => {
            total += it.price + len(it.tags)
          },
          Err(_) => (),
        }
      }
      print(f"{len(xs)} items, total {total}")
    },
    _ => print("not an array"),
  }
}
