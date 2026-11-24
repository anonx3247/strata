type Small = Int where it < 10

type Id = Small | String

fn show_id(i: Id) -> String {
  match i {
    String => "text " + i,
    _ => "small",
  }
}

fn main() {
  print(show_id(3))
  print(show_id("x"))
  let j: Id = 5
  print(j)
  mut k: Id = "a"
  k = 7
  print(k)
  k = "b" + "c"
  print(k)
}
