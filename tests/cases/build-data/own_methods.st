type Item = { name: String, price: Int }

impl Item {
  fn new(name: String) -> Item {
    Item { name, price: 0 }
  }

  fn label(self) -> String {
    f"{self.name}: {self.price}"
  }
}

fn main() {
  print(Item.new("cup").label())
}
