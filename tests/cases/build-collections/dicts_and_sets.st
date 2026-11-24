type Color =
  | Red
  | Green
  | Blue

distinct type UserId = Int

type Point = { x: Int, y: Int }

fn tally(words: List[String]) -> Dict[String, Int] {
  mut d: Dict[String, Int] = Dict {}
  for w in words {
    d = insert(d, w, (get(d, w) ?? 0) + 1)
  }
  d
}

fn main() {
  d := Dict { "b": 2, "a": 1, "c": 3 }
  print(d)
  print([len(d), len(Dict[Int, Int] {})])
  print(Dict[Int, String] {})
  print(dict([(3, "c"), (1, "a"), (3, "z"), (2, "b")]))
  print(insert(d, "a", 10))
  print(insert(d, "aa", 5))
  print(remove(d, "b"))
  print(remove(d, "zz"))
  print([get(d, "c"), get(d, "q")])
  print([contains(d, "a"), contains(d, "x")])
  print(keys(d))
  print(values(d))
  print(items(d))
  print(map_values(d, v -> float(v) * 1.5))
  print(map_values(d, v -> f"<{v}>"))
  for (k, v) in d {
    print(f"{k}={v}")
  }
  print(tally(split("the cat the hat the end cat", " ")))
  s := Set { 3, 1, 3, 2 }
  print(s)
  print([len(s), len(Set[String] {})])
  print([contains(s, 2), contains(s, 7)])
  print([insert(s, 0), insert(s, 2), remove(s, 1), remove(s, 9)])
  print(to_list(s))
  print([
    union(s, Set { 5, 0, 2 }),
    intersection(s, Set { 2, 3, 9 }),
    difference(s, Set { 2 }),
  ])
  for x in s {
    print(x)
  }
  print(s == Set { 1, 2, 3 })
  print(Dict { 1: "a" } == dict([(1, "a")]))
  print(d != remove(d, "a"))
  print(set(["pear", "apple", "fig", "apple"]))
  print(set([2.5, -1.0, 0.0, -0.5]))
  print(set([Blue, Red, Blue, Green]))
  print(set([UserId(10), UserId(9), UserId(100)]))
  print(set(
    [
      Point { x: 2, y: 1 },
      Point { x: 1, y: 5 },
      Point { x: 2, y: 1 },
    ],
  ))
  print(set([(2, "b"), (1, "z"), (2, "a")]))
  print(set([[2, 1], [1], [1, 0]]))
  print(set([Some(3), None, Some(1)]))
  print(Dict { Red: 1, Blue: 2 })
  print(Dict { (1, 2): "p", (0, 9): "q" })
  print(Set { Set { 2, 1 }, Set { 0 } })
  print(Dict { "x": Set { 1 }, "y": Set {} })
  nested := Dict { "k": Dict { 2: "two", 1: "one" } }
  print(nested)
  print(f"{d} {s}")
  print(to_string(s))
  print([d][0])
}
