trait Out {
  fn put(mut self, line: String)
  fn count(self) -> Int
}

type Log = { lines: List[String] }

impl Out for Log {
  fn put(mut self, line: String) {
    mut self.lines.push(line)
  }
  fn count(self) -> Int {
    len(self.lines)
  }
}

type Tally = { n: Int }

impl Out for Tally {
  fn put(mut self, line: String) {
    self.n = self.n + byte_len(line)
  }
  fn count(self) -> Int {
    self.n
  }
}

type Holder = { out: Out }

fn make(big: Bool) -> Out {
  if big {
    Log { lines: ["x"] }
  } else {
    Tally { n: 0 }
  }
}

fn pick(k: Int) -> Out {
  match k {
    0 => Log { lines: [] },
    _ => Tally { n: k },
  }
}

fn main() {
  h := Holder { out: Log { lines: ["x"] } }
  print(h.out.count())
  mut o: Out = Log { lines: [] }
  mut o.put("a")
  mut o.put("b")
  print(o.count())
  o = Tally { n: 5 }
  mut o.put("abc")
  print(o.count())
  let xs: List[Out] = [Log { lines: ["p"] }, Tally { n: 2 }]
  ys := xs + [Tally { n: 7 }]
  print(ys.map(x -> x.count()))
  print(make(true).count() + make(false).count())
  print(pick(0).count() + pick(3).count())
  let opt: Option[Out] = Some(Tally { n: 9 })
  match opt {
    Some(x) => print(x.count()),
    None => (),
  }
  mut h2 := Holder { out: Log { lines: ["x"] } }
  mut h2.out.put("a")
  match h2.out {
    Log { lines } => print(lines),
    _ => print("other"),
  }
  print("ok")
}
