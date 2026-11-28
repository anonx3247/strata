type Bag = { parts: List[String], name: String }

type Wrap = { text: String, bag: Bag }

type Quiet = { n: Int }

trait Holder {
  fn keep(self, w: Wrap) -> List[Wrap]
}

impl Holder for Quiet {
  fn keep(self, w: Wrap) -> List[Wrap] {
    []
  }
}

impl Bag {
  fn split(self, sep: String) -> List[Bag] {
    [self]
  }

  fn keep(self, w: Wrap) -> List[Wrap] {
    [w]
  }

  fn contains(self, s: String) -> List[Bag] {
    [self]
  }
}

fn pick(b: Bag, xs: List[String]) -> List[String] {
  if get(b.parts, 0) ?? "" == "x" {
    xs
  } else {
    []
  }
}

fn lens(b: Bag) -> List[Int] {
  mut out := []
  for p in b.parts {
    out += [byte_len(p)]
  }
  y := b.name
  out += [byte_len(y)]
  msg := f"{b.name}!"
  out += [byte_len(msg)]
  out
}

fn texts(all: List[Wrap]) -> List[List[String]] {
  mut out := []
  for v in all {
    if v.bag.parts.contains("x") {
      out += [["y"]]
    }
  }
  out
}

fn stash(mut into: List[Wrap], w: Wrap) -> Bool {
  mut into.push(w)
  true
}

trait Pair {
  fn take(self, w: Wrap) -> List[Wrap]
}

impl Pair for Quiet {
  fn take(self, w: Wrap) -> List[Wrap] {
    []
  }
}

impl Pair for Bag {
  fn take(self, w: Wrap) -> List[Wrap] {
    [w]
  }
}

fn taken(p: Pair, w: Wrap) -> List[Wrap] {
  p.take(w)
}

fn held(h: Holder, w: Wrap) -> List[Wrap] {
  h.keep(w)
}

fn main() {
  mut ss := []
  mut ls := []
  mut ws := []
  for i in 0..2 {
    bag := Bag { parts: [f"a{i}"], name: f"n{i}" }
    ss += [pick(bag, ["k"])]
  }
  for i in 0..2 {
    bag := Bag { parts: [f"b{i}"], name: f"m{i}" }
    ls += [lens(bag)]
  }
  for i in 0..2 {
    w := Wrap { text: f"c{i},x", bag: Bag { parts: [], name: "" } }
    ss += [w.text.split(",")]
  }
  for i in 0..2 {
    all := [Wrap { text: f"d{i}", bag: Bag { parts: [f"x{i}"], name: "" } }]
    ss += texts(all)
  }
  for i in 0..2 {
    o := Some(Wrap { text: f"g{i},y", bag: Bag { parts: [], name: "" } })
    match o {
      Some(u) => {
        ss += [u.text.split(",")]
      },
      None => (),
    }
  }
  for i in 0..2 {
    (t, n) := (Wrap { text: f"h{i},z", bag: Bag { parts: [], name: "" } }, i)
    ss += [t.text.split(",")]
  }
  for i in 0..2 {
    w := Wrap { text: f"q{i}", bag: Bag { parts: [], name: "" } }
    ws += held(Quiet { n: i }, w)
  }
  for i in 0..2 {
    w := Wrap { text: f"r{i}", bag: Bag { parts: [], name: "" } }
    if stash(mut ws, w) == false {
      print("no")
    }
  }
  for i in 0..2 {
    w := Wrap { text: f"s{i}", bag: Bag { parts: [], name: "" } }
    ws += taken(Quiet { n: i }, w)
  }
  print(ss)
  print(ls)
  print(len(ws))
}
