// Statement regions: values that live only in the statement making them go
// in the thread's region. Each function below is a way a value could
// outlive its statement, which must keep it counted.

type V = { ty: Int, op: String }

type G = { names: List[String] }

type W = { v: V, tag: String }

/// Keeps a field of its argument: the argument is kept.
fn keep_op(mut g: G, v: V) {
  mut g.names.push(v.op)
}

/// Gives back a string it was given.
fn either(a: String, b: String) -> String {
  if a == "" {
    b
  } else {
    a
  }
}

fn weigh(w: W) -> Int {
  byte_len(w.v.op) + byte_len(w.tag) + w.v.ty
}

fn first(xs: List[V]) -> String {
  match get(xs, 0) {
    Some(v) => v.op,
    None => "",
  }
}

fn total(n: Int) -> Int {
  mut t := 0
  for i in 0..n {
    t += weigh(W { v: V { ty: i, op: f"op{i}" }, tag: f"t{i % 3}" })
    if i == 5 {
      continue
    }
    t += byte_len(f"{i}:{i + 1}")
    if t > 100000000 {
      return t
    }
  }
  t
}

fn find(n: Int) -> Int {
  for i in 0..n {
    if weigh(W { v: V { ty: i, op: f"x{i}" }, tag: "k" }) > 12 {
      return i
    }
  }
  -1
}

fn main() {
  mut g := G { names: [] }
  for i in 0..4 {
    keep_op(mut g, V { ty: i, op: f"%x.{i}" })
  }
  print(join(g.names, ","))
  kept := either("", f"b{len(g.names)}")
  print(kept)
  alone := f"{either(f"a{1}", "z")}"
  print(alone)
  shared := [V { ty: 1, op: f"s{1}" }] + [V { ty: 2, op: "two" }]
  print(first(shared))
  print(total(20000))
  print(find(1000))
  print(first([V { ty: 3, op: f"o{3}" }, V { ty: 4, op: "four" }]))
}
