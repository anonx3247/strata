type Sq = { side: Int, name: String }

type Ci = { name: String, r: Int }

fn label(s: Sq | Ci) -> String {
  s.name
}

fn main() {
  print(label(Ci { name: "c", r: 2 }))
  print(label(Sq { side: 1, name: "q" }))
  mut s: Sq | Ci = Ci { name: "c", r: 2 }
  s.name = "renamed"
  print(s)
  mut t: Sq | Ci = Sq { side: 3, name: "s" }
  t.name += "!"
  print(t)
  mut pair := (1, "a")
  pair.1 = "b"
  pair.0 += 1
  print(pair)
  mut nested := ((1, 2), "x")
  nested.0.1 = 9
  print(nested)
}
