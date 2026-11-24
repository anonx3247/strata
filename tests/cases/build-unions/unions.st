type Sq = { side: Int }

fn describe(v: Int | String) -> String {
  match v {
    Int => f"number {v}",
    String => f"text {len(v.code_points())}",
  }
}

fn kind(v: Int | String | Sq | List[Int]) -> String {
  match v {
    0 => "zero",
    Positive => f"positive {v}",
    Int => "negative",
    "" => "empty text",
    String => f"text {v}",
    Sq => f"square {v.side}",
    [] => "empty list",
    [x, ..rest] => f"list from {x}, {len(rest)} more",
  }
}

fn pick(b: Bool) -> Int | String {
  if b {
    1
  } else {
    "one"
  }
}

fn main() {
  print(describe(4))
  print(describe("four"))
  for x in [0, 5, -2] {
    print(kind(x))
  }
  print(kind(""))
  print(kind("hey"))
  print(kind(Sq { side: 3 }))
  print(kind([]))
  print(kind([1, 2]))
  let xs: List[Int | String] = [1, "a", 2]
  print(xs)
  print(pick(true) == pick(true))
  print(pick(true) == pick(false))
  print(to_string(pick(false)))
  print(xs.map(x -> describe(x)))
  let u: Int | String = pick(false)
  print(u)
  print(u is String)
  print(u is Int)
}
