pub fn area(w: Int, h: Int) -> Int requires w >= 0 && h >= 0 {
  w * h
}

/// Twice the sum of the sides.
pub fn perimeter(w: Int, h: Int) -> Int {
  w + w + h + h
}

pub fn half(x: Int) -> Int {
  x / 2
}

pub fn third(x: Int) -> Int {
  x / 3
}

pub fn steps(x: Int) -> Int {
  a := x + 1
  b := a + 2
  c := b + 3
  d := c + 4
  e := d + 5
  f := e + 6
  e * f
}
