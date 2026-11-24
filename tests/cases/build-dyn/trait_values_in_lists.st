trait Shape {
  fn area(self) -> Int
  fn name(self) -> String {
    "shape"
  }
}

type Square = { side: Int }

type Rect = { w: Int, h: Int }

impl Shape for Square {
  fn area(self) -> Int {
    self.side * self.side
  }

  fn name(self) -> String {
    "square"
  }
}

impl Shape for Rect {
  fn area(self) -> Int {
    self.w * self.h
  }
}

fn total(shapes: List[Shape]) -> Int {
  sum(shapes.map(s -> s.area()))
}

fn main() {
  let shapes: List[Shape] = [Square { side: 2 }, Rect { w: 2, h: 5 }]
  print(total(shapes))
  print(shapes.map(s -> s.name()))
  print(shapes)
  let one: Shape = Square { side: 3 }
  print(one.area())
  print(one)
}
