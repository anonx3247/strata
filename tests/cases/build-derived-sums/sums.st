from std.typing use Choice, Union

/// A choice and its name.
derivable(Choice) trait Named {
  derived fn from_name(name: String) -> Option[Self] {
    comptime for v in Self.members {
      if to_lower(v.name ?? "") == name {
        return Some(v.make())
      }
    }
    None
  }
  derived fn name(self) -> String {
    match self {
      v => to_lower(v.name ?? "") for v in Self.members,
    }
  }
}

/// How many fields each variant has.
derivable(Union) trait Width
  requires len(Self.members) > 0 {
  derived fn width(self) -> Int {
    match self {
      v => len(v.fields) + len(v.elements) for v in Self.members,
    }
  }
}

type Level =
  | Quiet
  | Loud

derive Named for Level

type Shape =
  | Dot
  | Rect(Int, Int)
  | Circle { r: Int, label: String }

derive Width for Shape

fn main() {
  print(Level.from_name("loud"))
  print(Level.from_name("nope"))
  print(Loud.name())
  print(
    [Dot, Rect(1, 2), Circle { r: 1, label: "c" }].map(
      s -> s.width(),
    ),
  )
}
