trait Area {
  fn area(self) -> Int
    requires true
}

type Shape =
  | Square { side: Int }
  | Circle { r: Int }

impl Area for Shape {
  fn area(self) -> Int {
    0
  }
}

impl Area for Square {
  fn area(self) -> Int {
    self.side * self.side
  }
}

fn main() {
  let shapes: List[Shape] = [Square { side: 3 }, Circle { r: 2 }]
  print(shapes.map(s -> s.area()))
}
