type Item = { name: NonEmptyString, qty: Positive }

type Color =
  | Red
  | Green
  | Rgb(Int, Int, Int)

trait Store {
  fn count(self) -> Natural
  fn find(self, id: Int) -> Option[Item]
    requires id > 0
  fn all(self) -> List[Item]
    ensures len(it) <= 3
  fn color(self) -> Color
  fn code(self) -> String where matches(it, "[A-Z]{2}[0-9]{3}")
  fn save(mut self, i: Item) -> Result[(), String]
  fn rate(self) -> Float where 0.0 <= it && it <= 1.0
}

fn main() {
  mut s := mock Store
  for _ in 0..20 {
    n := s.count()
    assert n >= 0
    match s.find(3) {
      Some(i) => {
        assert i.name != "" && i.qty > 0
      },
      None => (),
    }
    assert len(s.all()) <= 3
    c := s.code()
    assert matches(c, "[A-Z]{2}[0-9]{3}")
    r := s.rate()
    assert 0.0 <= r && r <= 1.0
    k := s.color()
    assert k is Red || k is Green || k is Rgb
    res := mut s.save(Item { name: "a", qty: 1 })
    assert res is Ok || res is Err
  }
  print(s)
  print("all good")
  print(s.find(0))
}
