from std.json use Json

trait Monoid {
  fn empty() -> Self
  fn combine(self, other: Self) -> Self
  fn twice(self) -> Self {
    self.combine(self)
  }
  law left_identity(a: Self):
    Self.empty().combine(a) == a
}

type Money = { cents: Int }

impl Monoid for Money {
  fn empty() -> Money {
    Money { cents: 0 }
  }

  fn combine(self, other: Money) -> Money {
    Money { cents: self.cents + other.cents }
  }
}

type Max = { v: Int }

impl Monoid for Max {
  fn empty() -> Max {
    Max { v: 5 }
  }

  fn combine(self, other: Max) -> Max {
    if self.v > other.v {
      self
    } else {
      other
    }
  }
}

trait Sized {
  fn size(self) -> Int
    ensures it >= 0
}

impl Sized for Money {
  fn size(self) -> Int {
    self.cents
  }
}

impl Sized for Max {
  fn size(self) -> Int {
    if self.v < 0 {
      0 - self.v
    } else {
      self.v
    }
  }
}

type Item = { name: String, qty: Natural }

impl Item {
  fn new(name: String) -> Item {
    Item { name, qty: 0 }
  }

  fn total(self, price: Natural) -> Natural {
    self.qty * price
  }

  fn restock(self, n: Int) -> Item {
    Item { ..self, qty: self.qty + n }
  }
}

type Color = | Red | Green | Blue

derive Json for Color

fn sum_all(ms: List[Money]) -> Money {
  fold(ms, Money.empty(), (acc, m) -> acc.combine(m))
}
