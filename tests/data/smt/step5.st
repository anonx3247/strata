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

fn area(b: Box) -> Natural {
  b.size()
}

fn double(n: Natural) -> Natural {
  n * 2
}

fn halve(n: Int) -> Int
  requires n % 2 == 0
{
  n / 2
}

fn halves(xs: List[Int]) -> List[Int] {
  xs.map(halve)
}

fn pick[T](c: Bool, a: T, b: T) -> T {
  if c {
    a
  } else {
    b
  }
}

fn pick_pos(a: Pos, b: Pos) -> Pos {
  pick(a > b, a, b)
}

fn doubled(xs: List[Natural]) -> List[Natural]
  ensures len(it) == len(xs)
{
  xs.map(double)
}

fn quadruple(n: Natural) -> Natural
  ensures it == double(double(n))
{
  n * 4
}

fn wrong_quadruple(n: Natural) -> Natural
  ensures it == double(n)
{
  n * 4
}

fn halved(n: Int) -> Int
  requires n % 2 == 0
  ensures it == halve(n)
{
  n / 2
}
