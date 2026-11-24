fn describe(v: List[Int] | String) -> String {
  match v {
    List[Int] => f"list {len(v)}",
    String => "text",
  }
}

fn main() {
  mut u: Int | String = 3
  u = "text"
  print(u)
  let w: Int | String = 3
  print(w)
  print(describe([1, 2]))
  print(describe("x"))
  let e: List[Int] | String = []
  match e {
    List[Int] => print("empty"),
    String => print("text"),
  }
}
