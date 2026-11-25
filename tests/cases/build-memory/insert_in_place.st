type Entry = { name: String, n: Int }

type State = { seen: Set[Int], names: Dict[String, Entry], count: Int }

fn note(mut s: State, k: String, n: Int) {
  mut s.names.insert(k, Entry { name: k + "!", n })
  mut s.seen.insert(n)
  s.count += 1
}

fn main() {
  mut d := Dict[String, Int] {}
  mut i := 0
  while i < 2000 {
    mut d.insert("k" + to_string(i % 700), i)
    i += 1
  }
  (k5, k699) := ("k5", "k699")
  print(f"{len(d)} {get(d, k5) ?? -9} {get(d, k699) ?? -9}")
  // a copy keeps its value
  old := d
  mut d.insert("new", -1)
  mut d.insert("k5", 5)
  new := "new"
  print(
    f"{len(old)} {len(d)} {get(old, k5) ?? -9} {get(d, k5) ?? -9} {contains(old, new)}",
  )
  // so does one in a list
  ds := [d]
  mut d.insert("later", 0)
  print(f"{len(ds[0])} {len(d)}")
  // a constant dictionary
  mut c := Dict[String, Int] { "a": 1, "b": 2 }
  mut c.insert("c", 3)
  mut c.insert("a", 10)
  e := Dict[String, Int] { "a": 1, "b": 2 }
  print(f"{c} {e}")
  // a value mentioning the dictionary itself
  mut dd := Dict[String, Int] { "x": 1 }
  mut dd.insert("size", len(dd))
  mut dd.insert("size2", len(dd))
  print(dd)
  // sets, through a variable and a method call
  mut seen := Set[Int] {}
  for n in [5, 3, 9, 3, 1, 5, 7] {
    mut seen.insert(n)
    mut seen.insert(n * 100)
  }
  print(seen)
  // fields of a record, through a `mut` parameter
  mut s := State {
    seen: Set[Int] {},
    names: Dict[String, Entry] {},
    count: 0,
  }
  for w in ["b", "a", "c", "a"] {
    note(mut s, w, len(w.bytes()) + s.count)
  }
  t := s
  note(mut s, "z", 99)
  a := get(s.names, "a") ?? Entry { name: "?", n: -9 }
  print(
    f"{s.count} {len(s.names)} {a.name} {a.n} {s.seen} {t.count} {len(t.names)}",
  )
  print("done")
}
