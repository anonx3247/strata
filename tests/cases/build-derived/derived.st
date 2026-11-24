from std.typing use Record, Field

/// Values from text, field by field.
derivable(Record) trait FromText {
  /// The fields' names.
  derived fn names(self) -> List[String] {
    comptime Self.fields.map(f -> f.name)
  }
  /// A value with every field at its zero.
  derived fn zero() -> Self {
    Self.make(f -> zero_of(f))
  }
  /// A value read from one text per field.
  derived fn parse(texts: List[String]) -> Result[Self, String] {
    Self.try_make(f -> read(f, texts))
  }
}

fn zero_of(comptime f: Field) -> f.type {
  match comptime f.type {
    Int => 0,
    String => "",
    Bool => false,
  }
}

fn read(comptime f: Field, texts: List[String]) -> Result[f.type, String] {
  text := get(texts, index_of_name(comptime f.name)) ?? ""
  match comptime f.type {
    Int => match parse_int(text) {
      Some(n) => Ok(n),
      None => Err(f"{f.name}: `{text}` is not a number"),
    },
    String => Ok(text),
    Bool => Ok(text == "yes"),
  }
}

fn index_of_name(name: String) -> Int {
  if name == "x" {
    0
  } else if name == "label" {
    1
  } else {
    2
  }
}

type Point = { x: Int, label: String, on: Bool }

derive FromText for Point

fn main() {
  print(Point.zero().names())
  print(Point.zero())
  print(Point.parse(["3", "here", "yes"]))
  print(Point.parse(["three", "here", "yes"]))
}
