type Pos = Int where it > 0

trait Sized {
  fn size(self) -> Int
    ensures it >= 0
}

type Box = { w: Pos, h: Pos }

impl Sized for Box {
  fn size(self) -> Int {
    self.w * self.h
  }
}

fn bigger(a: Box, b: Box) -> Bool
  ensures it ==> a.size() > 0
{
  a.size() > b.size()
}

fn nonneg(a: Box) -> Bool
  ensures it
{
  a.size() >= 0
}
