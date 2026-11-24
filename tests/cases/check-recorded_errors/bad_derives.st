from std.json use Json

type Bad = { type: Int }

derive Json for Bad

type Pairs =
  | Two(Int, Int)
  | None2

derive Json for Pairs

type Inner = { x: Int }

type Outer = { inner: Inner }

derive Json for Outer

fn main() { print(1) }
