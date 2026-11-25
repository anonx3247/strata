// `push(xs, 3)` on a `List[U | Int]`: the element is made the union's value,
// as a list literal's elements are (it was stored as a bare `Int`, and
// printing the list crashed)

type A = | X | Y
type B = | Z
type U = A | B
type E

fn f() -> Result[List[U | Int], E] {
  mut xs: List[U | Int] = []
  xs = push(xs, X)
  xs = push(xs, 3)
  xs = push(xs, Z)
  Ok(xs)
}

fn words() -> List[String | Int] {
  mut xs: List[String | Int] = []
  xs = push(xs, "a")
  xs = push(xs, 2)
  xs
}

fn main() {
  print(f())
  print(words())
  match f() {
    Ok(xs) => print(len(xs)),
    Err(_) => print("none"),
  }
}
