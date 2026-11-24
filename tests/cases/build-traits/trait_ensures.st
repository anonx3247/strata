trait Describe {
  fn describe(self) -> String
    ensures it != ""
}

type Thing = { name: String }

impl Describe for Thing {
  fn describe(self) -> String {
    self.name
  }
}

fn main() {
  print(Thing { name: "a" }.describe())
  print(Thing { name: "" }.describe())
}
