type Pos = Int where it > 0

type Range = { lo: Int, hi: Int } where it.lo <= it.hi

type Point = { x: Int, y: Int }

type Shape =
  | Circle { radius: Int }
  | Rect { w: Int, h: Int }
  | Dot

type Box = { size: Pos, label: String }

distinct type Meters = Int

type Tree =
  | Leaf
  | Node(Tree, Int, Tree)

fn width(r: Range) -> Natural {
  r.hi - r.lo
}

fn make_range(a: Int, b: Int) -> Range {
  if a <= b {
    Range { lo: a, hi: b }
  } else {
    Range { lo: b, hi: a }
  }
}

fn bad_range(a: Int, b: Int) -> Range {
  Range { lo: a, hi: b }
}

fn widen(r: Range) -> Range {
  Range { ..r, hi: r.hi + 1 }
}

fn shrink(r: Range) -> Range {
  Range { ..r, hi: r.hi - 1 }
}

fn area(s: Shape) -> Natural {
  match s {
    Circle { radius } => radius * radius * 3,
    Rect { w, h } => w * h,
    Dot => 0,
  }
}

fn safe_area(s: Shape) -> Natural {
  match s {
    Circle { radius } => radius * radius * 3,
    Rect { w, h } => if w >= 0 && h >= 0 {
      w * h
    } else {
      0
    },
    Dot => 0,
  }
}

fn is_dot(s: Shape) -> Bool
  ensures it == (s is Dot)
{
  match s {
    Dot => true,
    _ => false,
  }
}

fn mid(p: Point, q: Point) -> Point {
  Point { x: (p.x + q.x) / 2, y: (p.y + q.y) / 2 }
}

fn swap(t: (Int, String)) -> (String, Int) {
  (s, n) := (t.1, t.0)
  (s, n)
}

fn first_pos(t: (Int, Int)) -> Pos {
  match t {
    (a, _) => if a > 0 {
      a
    } else {
      1
    },
  }
}

fn grow(b: Box) -> Box {
  Box { ..b, size: b.size + 1 }
}

fn bad_box(n: Int) -> Box {
  Box { size: n, label: "b" }
}

fn opt_or(o: Option[Int]) -> Int {
  o ?? 0
}

fn opt_pos(o: Option[Pos]) -> Pos {
  match o {
    Some(n) => n,
    None => 1,
  }
}

fn opt_bad(o: Option[Int]) -> Pos {
  match o {
    Some(n) => n,
    None => 1,
  }
}

fn parse_pos(n: Int) -> Result[Pos, String] {
  if n > 0 {
    Ok(n)
  } else {
    Err("not positive")
  }
}

fn twice_pos(n: Int) -> Result[Pos, String] {
  p := parse_pos(n)!
  Ok(p * 2)
}

fn add_opt(a: Option[Int], b: Option[Int]) -> Option[Int] {
  x := a?
  y := b?
  Some(x + y)
}

fn fallback(n: Int) -> Pos {
  parse_pos(n) except e { 5 }
}

fn meters(m: Meters) -> Meters {
  Meters(m.value + 1)
}

fn depth(t: Tree) -> Natural {
  match t {
    Leaf => 0,
    Node(_, v, _) => if v > 0 {
      v
    } else {
      0
    },
  }
}

fn same_point(p: Point) -> Bool
  ensures it
{
  p == Point { x: p.x, y: p.y }
}

fn shapes_eq(a: Shape, b: Shape) -> Bool
  ensures it == (a == b)
{
  a == b
}

fn pick(c: Bool, p: Point, q: Point) -> Point
  ensures it == p || it == q
{
  if c {
    p
  } else {
    q
  }
}

fn nested(s: Shape) -> Int {
  match s {
    Rect { w, h: 0 } => w,
    Rect { w: 1, h } => h,
    Circle { radius } | Rect { w: radius, .. } => radius,
    _ => 0,
  }
}

fn let_pattern(p: Point) -> Int {
  Point { x, y } := p
  x + y
}

fn sign(n: Int) -> Int {
  match n {
    where it > 0 => 1,
    where it < 0 => -1,
    _ => 0,
  }
}

fn parity(n: Int) -> String {
  match n {
    where it % 2 == 0 => "even",
    where it % 2 == 1 => "odd",
  }
}

fn thirds(n: Int) -> Int {
  match n {
    where it % 3 == 0 => 0,
    where it % 3 == 1 => 1,
  }
}

fn radius_of(s: Shape) -> Pos {
  match s {
    Circle { radius: where it > 0 } => s.radius,
    _ => 1,
  }
}

fn generic_eq[T](a: T, b: T) -> Bool {
  a == b
}
