from std.ffi use Python
from std.json use Json

/// A point on the plane.
type Point = { x: Float, y: Float }

derive Json for Point

/// A shape.
type Shape =
  | Circle { center: Point, r: Float }
  | Square(Float)
  | Dot

derive Json for Shape

type Level = Quiet | Loud

derive Json for Level

type Count = Int where it > 0

/// Why a division failed.
type DivError =
  | ByZero
  | Overflow { message: String }

derive Json for DivError

/// The sum of two numbers.
pub fn add(a: Int, b: Int) -> Int {
  a + b
}

/// The middle of two points.
pub fn middle(p: Point, q: Point) -> Point {
  Point { x: (p.x + q.x) / 2.0, y: (p.y + q.y) / 2.0 }
}

/// A shape's area.
pub fn area(s: Shape) -> Float {
  match s {
    Circle { r, .. } => 3.0 * r * r,
    Square(a) => a * a,
    Dot => 0.0,
  }
}

/// `n` times `x`, or `x` once.
pub fn repeat(x: String, n: Option[Count]) -> List[String] {
  [x for _ in 0..(n ?? 1)]
}

/// `a / b`.
pub fn divide(a: Int, b: Int) -> Result[Int, DivError] {
  if b == 0 {
    Err(ByZero)
  } else {
    Ok(a / b)
  }
}

/// Louder.
pub fn louder(_l: Level) -> Level {
  Loud
}

/// Says hello.
pub fn hello() {
  print("hello from Strata")
}

derive Python for add

derive Python for middle

derive Python for area

derive Python for repeat

derive Python for divide

derive Python for louder

derive Python for hello
