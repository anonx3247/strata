type P = { x: Int, name: String }

type Box = { p: P, tag: String }

fn norm(p: P) -> Int {
  p.x * 2 + byte_len(p.name)
}

fn wrap(p: P) -> Box {
  Box { p, tag: "w" }
}

fn first(a: P, b: P) -> Int {
  a.x + b.x
}

fn main() {
  // constructors as arguments: on the stack when the callee only reads them
  mut total := 0
  mut kept := []
  mut i := 0
  while i < 1000 {
    total += norm(P { x: i, name: "n" + to_string(i) })
    // a variable holding the call's result, not the argument
    c := norm(P { x: i, name: "c" })
    total += c
    total += first(P { x: 1, name: to_string(i) }, P { x: i, name: "b" })
    if i % 100 == 0 {
      mut kept.push(wrap(P { x: i, name: "kept" + to_string(i) }))
    }
    i += 1
  }
  print(f"{total} {len(kept)} {kept[3].p.name}")
  // removing in place
  mut d := Dict[String, Int] {}
  for k in 0..300 {
    mut d.insert("k" + to_string(k), k)
  }
  old := d
  for k in 0..300 {
    if k % 3 == 0 {
      mut d.remove("k" + to_string(k))
    }
  }
  mut d.remove("absent")
  print(f"{len(d)} {len(old)}")
  mut s := Set[Int] { 1, 2, 3 }
  mut s.remove(2)
  mut s.remove(9)
  t := s
  mut s.remove(1)
  print(f"{s} {t}")
  mut c := Dict[String, Int] { "a": 1, "b": 2 }
  mut c.remove("a")
  e := Dict[String, Int] { "a": 1, "b": 2 }
  print(f"{c} {e}")
  print("done")
}
