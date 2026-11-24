from std.json use Json, parse, stringify
from std.regex use full_match

fn main() {
  match parse("{\"a\": [1, 2.5, true, null, \"s\"], \"b\": {}}") {
    Ok(j) => print(stringify(j)),
    Err(e) => print(e),
  }
  print(parse("[1,"))
  print(full_match("ab1", "[a-z]+[0-9]"))
  print(full_match("ab", "[a-z]+[0-9]"))
  print(full_match("x", "("))
}
