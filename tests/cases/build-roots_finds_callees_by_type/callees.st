// Calls whose callee is a method of a program type named like a built-in,
// and a local closure named like a function of the program.

type Bag = { parts: List[String] }

impl Bag {
  /// Gives back the bag itself, in a list.
  fn split(self, sep: String) -> List[Bag] {
    [self]
  }
}

/// Keeps nothing of its argument.
fn pick(xs: List[String]) -> Int {
  len(xs)
}

fn main() {
  mut kept := []
  mut bags := []
  pick := (xs: List[String]) -> xs
  for i in 0..3 {
    let b: Bag = Bag { parts: [f"b{i}"] }
    xs := b.split(",")
    zs := split(f"s{i},t", ",")
    ps := pick([f"p{i}"])
    mut bags.push(xs)
    mut kept.push(zs)
    mut kept.push(ps)
  }
  print(len(bags))
  print(kept)
}
