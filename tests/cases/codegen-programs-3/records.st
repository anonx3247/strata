type Point = { x: Int, y: Int }

type Line = { from: Point, to: Point, label: String = "line" }

fn shift(p: Point, dx: Int) -> Point {
    Point { ..p, x: p.x + dx }
}

fn length2(l: Line) -> Int {
    dx := l.to.x - l.from.x
    dy := l.to.y - l.from.y
    dx * dx + dy * dy
}

fn main() {
    p := Point { x: 1, y: 2 }
    q := shift(p, 3)
    print(p)
    print(q)
    print(q.x + q.y)
    l := Line { from: p, to: q }
    print(l)
    print(length2(l))
    print(f"{l.label}: {l.from} -> {l.to}")
    print(p == Point { x: 1, y: 2 })
    print(p == q)
    print(p != q)
    pair := (p.x, "two", 3.5, true)
    print(pair)
    print(pair.1)
    (a, b, _, d) := pair
    print(f"{a} {b} {d}")
    print((1, (2, 3)) == (1, (2, 3)))
}
