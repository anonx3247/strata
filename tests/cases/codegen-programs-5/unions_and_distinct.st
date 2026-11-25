type Circle = { r: Float }

type Shape = Circle | Square { side: Float } | Dot

type Id = Int | String

distinct type UserId = Int

distinct type Label = String

fn area(s: Shape) -> Float {
    match s {
        Circle { r } => 3.0 * r * r,
        Square { side } => side * side,
        Dot => 0.0,
    }
}

fn describe(id: Id) -> String {
    match id {
        Int => f"number {id + 1}",
        String => f"text {id}",
    }
}

fn small(n: Natural) -> Natural {
    n * 2
}

fn main() {
    let shapes: List[Shape] = [Circle { r: 1.0 }, Square { side: 2.0 }, Dot]
    for s in shapes {
        print(area(s))
    }
    print(shapes)
    let a: Id = 41
    let b: Id = "hi"
    print(describe(a))
    print(describe(b))
    print(a)
    print(b)
    print([a, b])
    print(a == b)
    let c: Id = 41
    print(a == c)
    print([b, a, c].sorted())
    if b is String {
        print(f"{b}!")
    }
    print(a is Int)
    u := UserId(7)
    print(u)
    print(u.value + 1)
    print(u == UserId(7))
    print(([UserId(9), UserId(3)]).sorted())
    l := Label("x")
    print(f"{l} {l.value}")
    print(small(4))
    let one: Shape = Dot
    print(one == Dot)
}
