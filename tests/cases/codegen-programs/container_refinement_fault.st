fn first_pos(xs: List[Int]) -> Option[Positive] {
  for x in xs {
    if x > 0 {
      return Some(x)
    }
  }
  None
}

fn main() {
  print(first_pos([-1, 3]))
  let s: Set[Natural] = Set { 1, 2 }
  print(s)
  let d: Dict[String, Positive] = Dict { "a": 1 }
  print(d)
  let r: Result[Natural, String] = Ok(4)
  print(r)
  let bad: Dict[String, Positive] = Dict { "b": 0 }
  print(bad)
}
