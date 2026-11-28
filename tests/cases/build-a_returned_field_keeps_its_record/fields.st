type Bag = { parts: List[String], name: String }

type Pair = { inner: Bag, n: Int }

fn parts_of(self: Bag) -> List[String] {
  self.parts
}

fn inner_parts(p: Pair) -> List[String] {
  p.inner.parts
}

fn chosen(b: Bag, first: Bool) -> List[String] {
  if first {
    b.parts
  } else {
    match b.name {
      "" => [],
      _ => b.parts,
    }
  }
}

fn kept_in(b: Bag) -> (List[String], Int) {
  (b.parts, 1)
}

fn unpacked(b: Bag) -> List[String] {
  match b {
    Bag { parts, .. } => parts,
  }
}

fn joined(b: Bag) -> List[String] {
  b.parts + ["z"]
}

fn more(xs: List[String]) -> List[String] {
  xs + ["y"]
}

fn size(b: Bag) -> Int {
  len(b.parts) + byte_len(b.name)
}

fn main() {
  mut out := []
  for i in 0..3 {
    bag := Bag { parts: [f"a{i}", f"b{i}"], name: f"n{i}" }
    out += [parts_of(bag)]
  }
  for i in 0..2 {
    pair := Pair { inner: Bag { parts: [f"p{i}"], name: "x" }, n: i }
    out += [inner_parts(pair)]
  }
  for i in 0..2 {
    bag := Bag { parts: [f"c{i}"], name: f"m{i}" }
    out += [chosen(bag, i == 0)]
  }
  for i in 0..2 {
    bag := Bag { parts: [f"t{i}"], name: "y" }
    out += [kept_in(bag).0]
  }
  for i in 0..2 {
    bag := Bag { parts: [f"u{i}"], name: "w" }
    out += [unpacked(bag)]
  }
  for i in 0..2 {
    bag := Bag { parts: [f"j{i}"], name: "q" }
    out += [joined(bag)]
  }
  for i in 0..2 {
    xs := [f"x{i}"]
    out += [more(xs)]
  }
  mut total := 0
  for i in 0..2 {
    bag := Bag { parts: [f"s{i}"], name: f"v{i}" }
    total += size(bag)
  }
  print(total)
  held := {
    bag := Bag { parts: [f"k{len(out)}"], name: "z" }
    parts_of(bag)
  }
  mut churn := []
  for j in 0..50 {
    churn += [Bag { parts: [f"junk{j}junk"], name: f"zz{j}" }]
  }
  print(len(churn))
  print(out)
  print(held)
}
