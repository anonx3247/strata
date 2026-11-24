type Name = { first: String, last: String }

type Person = { name: Name, age: Int, tags: List[String] }

// borrows `n`: a field read
fn full(n: Name) -> String {
  n.first + " " + n.last
}

// borrows `p` (passes it on to a function that borrows it)
fn label(p: Person) -> String {
  full(p.name) + "/" + to_string(len(p.tags))
}

// keeps `p`
fn keep(p: Person) -> List[Person] {
  [p]
}

// a value on the stack left by `return`
fn find(names: List[String], wanted: String) -> Int {
  mut i := 0
  while i < len(names) {
    n := Name { first: names[i], last: "L" }
    if n.first == wanted {
      return i
    }
    i += 1
  }
  -1
}

fn main() {
  mut total := 0
  mut kept := []
  mut i := 0
  while i < 1000 {
    i += 1
    n := Name { first: "Ada" + to_string(i), last: "Lovelace" }
    p := Person {
      name: Name { first: "x", last: to_string(i) },
      age: i * 4611686018427387904,
      tags: ["a", to_string(i)],
    }
    if i % 3 == 0 {
      continue
    }
    total += byte_len(full(n)) + byte_len(label(p))
    if i == 500 {
      kept += keep(p)
    }
    q := Person { name: n, age: i, tags: [] }
    match q {
      Person { age, .. } => {
        total += age % 5
      },
    }
    if i == 998 {
      break
    }
  }
  print(total)
  print(len(kept))
  names := ["Ada1", "Ada7", "Bob"]
  at := find(names, "Ada7")
  print(f"found Ada7 at {at}")
}
