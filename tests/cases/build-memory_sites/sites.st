// A program touching each kind of counting operation `--memory-sites`
// counts: blocks on the heap, in an arena and on the stack, dups and
// releases, updates in place that test the count, a list and a string
// extended.

from std.memory use arena

type Point = { x: Int, y: Int }

type Path = { _name: String, points: List[Point], steps: Int }

fn grow(n: Int) -> Path {
  mut p := Path { _name: "p", points: [], steps: 0 }
  for i in 0..n {
    mut p.points.push(Point { x: i, y: i * i })
    p.steps += 1
  }
  p
}

fn label(n: Int) -> String {
  mut s := ""
  for i in 0..n {
    s += to_string(i % 10)
  }
  s
}

fn norm(p: Point) -> Int {
  p.x * p.x + p.y * p.y
}

fn main() {
  p := grow(10)
  q := p
  mut r := p
  r.steps = 0
  mut xs := [1, 2, 3]
  ys := xs
  xs[0] = 9
  a := arena(() -> grow(5))
  mut total := 0
  for i in 0..4 {
    c := Point { x: i, y: 1 }
    total += norm(c)
  }
  print(f"{len(q.points)} {r.steps} {xs} {ys} {a.steps} {label(12)} {total}")
}
