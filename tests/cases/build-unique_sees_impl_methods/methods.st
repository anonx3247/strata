type Bag = { n: Int }

impl Bag {
  fn squares(self) -> List[Int] {
    mut out: List[Int] = []
    for i in 0..self.n {
      mut out.push(i * i)
    }
    out
  }
}

fn main() {
  b := Bag { n: 4 }
  print(b.squares())
}
