type Shape =
  | Square { side: Int }
  | Circle { r: Int }

trait Area {
  fn area(self) -> Int
  fn grow(mut self)
}

impl Area for Shape {
  fn area(self) -> Int {
    match self {
      Square { side } => side * side,
      Circle { r } => 3 * r * r,
    }
  }

  fn grow(mut self) {
    self = match self {
      Square { side } => Square { side: side + 1 },
      Circle { r } => Circle { r: r + 1 },
    }
  }
}

type Named = { name: String, tags: List[String] }

type Box = { shape: Area, label: String }

fn first_long(xs: List[String]) -> Option[String] {
  for x in xs {
    if len(x.code_points()) > 3 {
      return Some(x)
    }
  }
  None
}

fn parse_all(xs: List[String]) -> Result[List[Int], String] {
  mut out := []
  for x in xs {
    n := parse_int(x) or { return Err(f"bad {x}") }
    out = push(out, n)
  }
  Ok(out)
}

fn total(xs: List[String]) -> Result[Int, String] {
  ns := parse_all(xs)!
  Ok(ns.fold(0, (a, b) -> a + b))
}

fn rename(mut n: Named, to: String) {
  n.name = to
  n.tags = push(n.tags, "renamed")
}

fn describe(xs: List[Int]) -> String {
  match xs {
    [] => "empty",
    [x] => f"one {x}",
    [1, b, ..rest] => f"one then {b} then {rest}",
    [a, ..rest] => f"starts {a}, {len(rest)} more",
  }
}

fn pick(s: Shape) -> String {
  match s {
    Square { side: 1 } | Circle { r: 1 } => "unit",
    Square { side } | Circle { r: side } => f"size {side}",
  }
}

fn main() {
  // closures capturing strings and lists
  prefix := "item-"
  names := ["a", "bb", "cccc", "ddddd"]
  labelled := names.map(n -> prefix + n)
  print(labelled)
  print(labelled.filter(n -> len(n.code_points()) > 6))
  adder := (x: Int) -> x + len(prefix.code_points())
  print(adder(1))
  // early return inside a loop, Option
  print(first_long(names))
  print(first_long(["a"]))
  // ? / ! and `or { return }`
  print(total(["1", "2", "3"]))
  print(total(["1", "x"]))
  // mut parameters, fields
  mut n := Named { name: "old", tags: ["t"] }
  rename(mut n, "new")
  print(n)
  mut holder := Named { name: "h", tags: [] }
  holder.tags = n.tags
  holder.name = "inner"
  print(holder)
  print(n)
  // trait values in a list and a field, with a mut self call
  let shapes: List[Area] = [Square { side: 2 }, Circle { r: 1 }]
  print(shapes.map(s -> s.area()))
  mut b := Box { shape: Square { side: 3 }, label: "b" }
  mut b.shape.grow()
  print(b.shape.area())
  mut sh: Area = Circle { r: 2 }
  mut sh.grow()
  print(sh.area())
  // list patterns, or-patterns
  print(describe([]))
  print(describe([7]))
  print(describe([1, 2, 3, 4]))
  print(describe([5, 1, 0]))
  print(pick(Square { side: 1 }))
  print(pick(Circle { r: 4 }))
  // loops with break / continue and locals
  mut acc := []
  mut i := 0
  while true {
    i += 1
    s := f"n{i}"
    if i % 2 == 0 {
      continue
    }
    if i > 7 {
      break
    }
    acc = push(acc, s)
  }
  print(acc)
  for w in names {
    t := w + "!"
    if len(w.code_points()) == 2 {
      continue
    }
    if len(w.code_points()) == 5 {
      break
    }
    print(t)
  }
  // dicts and sets
  mut d := Dict { "a": 1 }
  d = insert(d, "b", 2)
  d = insert(d, "a", 3)
  print(d)
  print(get(d, "a"))
  print(keys(d))
  s := Set { "x", "y" }
  print(insert(s, "z"))
  print(contains(s, "x"))
  // strings
  text := "  Hello, World  "
  print(trim(text).to_upper())
  print(split("a,b,c", ","))
  print(replace("aXbXc", "X", "--"))
  print(f"{3.14159:.2} [{42:>6}] [{"ab":<4}]")
  print(chars("héllo").reverse())
  print(parse_float(" 2.5 "))
  print(lines("one\ntwo"))
  // reassigning in a loop
  mut words := []
  for k in 0..5 {
    words += [f"w{k}"]
  }
  print(words)
  print(words[1..3])
  print(words.sort_by(w -> w).reverse())
}
