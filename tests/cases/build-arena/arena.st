// Arenas: what `arena(f)` gives back is copied out of the arena, which is
// then freed; `arena_kept(f)` keeps its arena.

from std.memory use arena, arena_kept

type Shape =
  | Circle(Float)
  | Rect { w: Int, h: Int, name: String }

type Box = { label: String, items: List[String], big: Int, kids: List[Shape] }

fn work(n: Int) -> Box {
  mut items := []
  for i in 0..n {
    items = push(items, f"item {i}")
  }
  Box {
    label: "box " + to_string(n),
    items,
    big: 12345678901234567890123 * n,
    kids: [Circle(1.5), Rect { w: n, h: 2, name: "r" + to_string(n) }],
  }
}

fn table(n: Int) -> Dict[String, List[Int]] {
  mut m := Dict[String, List[Int]] {}
  for i in 0..n {
    m = insert(m, f"k{i}", [i, i * 2])
  }
  m
}

fn pick(n: Int) -> Int | String | Shape {
  if n > 5 {
    Rect { w: n, h: n, name: "u" + to_string(n) }
  } else if n > 2 {
    "big " + to_string(n)
  } else {
    n * 100000000000000000000
  }
}

fn main() {
  outside := work(2)
  b := arena(() -> work(50))
  print(b.label)
  print(len(b.items))
  print(get(b.items, 49) ?? "")
  print(b.big)
  match get(b.kids, 1) {
    Some(Rect { name, .. }) => print(name),
    _ => (),
  }
  d := arena(() -> table(10))
  print(get(d, "k7") ?? [])
  t := arena(() -> ("a" + to_string(len(b.items)), [1, 2, 3]))
  print(t.0)
  print(t.1)
  o := arena(() -> get(work(4).items, 2))
  print(o)
  shared := arena(() -> (outside.label, outside.items, "x" + outside.label))
  print(shared)
  k := arena_kept(() -> work(3))
  print(k.label)
  nested := arena(() -> {
    inner := arena(() -> work(5))
    inner.label + "!"
  })
  print(nested)
  n := arena(() -> len(work(10).items))
  print(n)
  r := arena(() -> scope {
    x := spawn work(4)
    y := spawn work(6).label
    (await x).label + " " + await y
  })
  print(r)
  mut total := 0
  for i in 0..200 {
    s := arena(() -> work(i % 7).items)
    total += len(s)
  }
  print(total)
  print(arena(() -> [pick(1), pick(4), pick(9)]))
}
