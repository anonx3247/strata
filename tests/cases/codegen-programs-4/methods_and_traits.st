type P = { x: Int, y: Int }

type Circle = { r: Float }

type Square = { side: Float }

type Box[T] = { item: T }

trait Shape {
    fn area(self) -> Float

    fn name(self) -> String

    fn describe(self) -> String {
        f"{self.name()} of area {self.area()}"
    }
}

trait Named {
    fn label(self) -> String
}

impl P {
    fn origin() -> P {
        P { x: 0, y: 0 }
    }

    fn plus(self, o: P) -> P {
        P { x: self.x + o.x, y: self.y + o.y }
    }

    fn norm1(self) -> Int {
        abs(self.x) + abs(self.y)
    }
}

impl Display for P {
    fn to_string(self) -> String {
        f"<{self.x}, {self.y}>"
    }
}

impl Shape for Circle {
    fn area(self) -> Float {
        3.0 * self.r * self.r
    }

    fn name(self) -> String {
        "circle"
    }
}

impl Shape for Square {
    fn area(self) -> Float {
        self.side * self.side
    }

    fn name(self) -> String {
        "square"
    }

    fn describe(self) -> String {
        f"a square, side {self.side}"
    }
}

impl Named for Circle {
    fn label(self) -> String {
        "C"
    }
}

impl[T] Box[T] {
    fn get(self) -> T {
        self.item
    }

    fn map[U](self, f: (T) -> U) -> Box[U] {
        Box { item: f(self.item) }
    }
}

fn total_area[T: Shape](xs: List[T]) -> Float {
    fold(xs, 0.0, (acc, s) -> acc + s.area())
}

fn biggest(shapes: List[Shape]) -> String {
    mut best := ""
    mut most := -1.0
    for s in shapes {
        if s.area() > most {
            most = s.area()
            best = s.describe()
        }
    }
    best
}

fn main() {
    p := P.origin().plus(P { x: 3, y: -4 })
    print(p)
    print(f"p = {p}, norm {p.norm1()}")
    print([p, P { x: 1, y: 1 }])
    print(Circle { r: 1.0 }.describe())
    print(Square { side: 2.0 }.describe())
    print(Circle { r: 2.0 }.label())
    b := Box { item: 21 }
    print(b.get())
    print(b.map(x -> x * 2).get())
    print(b.map(x -> f"#{x}"))
    print(total_area([Circle { r: 1.0 }, Circle { r: 2.0 }]))
    let shapes: List[Shape] = [Circle { r: 1.0 }, Square { side: 3.0 }, Circle { r: 0.5 }]
    print(biggest(shapes))
    for s in shapes {
        print(s.name())
    }
    print(shapes)
    let one: Shape = Square { side: 1.0 }
    let same: Shape = Square { side: 1.0 }
    print(one == same)
    print(one.describe())
}
