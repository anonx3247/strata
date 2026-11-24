type Small = Int where it < 10

type Id = Small | String

fn show_id(i: Id) -> String {
  match i {
    String => "text " + i,
    _ => "small",
  }
}

fn main() {
  print(show_id(30))
  print(show_id("x"))
  let j: Id = 50
  print(j)
}
