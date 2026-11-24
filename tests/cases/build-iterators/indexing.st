type Naturals = { at: Int }

impl Iterator[Int] for Naturals {
  fn next(mut self) -> Option[Int] {
    self.at += 1
    Some(self.at - 1)
  }
}

fn main() {
  s := "héllo 👋🏽!"
  print(len(s.bytes()))
  print(len(s.code_points()))
  print(len(s.graphemes()))
  print(s.graphemes()[6])
  print(get(s.graphemes(), 9))
  print(join(s.graphemes()[1..3], ""))
  print(join(s.graphemes()[5..], "|"))
  print(s.code_points()[1])
  print(Naturals { at: 0 }.drop(3).take(4).to_list())
  print(len(Naturals { at: 0 }.take(5)))
  print(take(Naturals { at: 10 }, 2).to_list())
  print([1, 2, 3][1..])
  print(len([1, 2]))
  print(get([1, 2], 5))
}
