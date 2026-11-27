lemma double_le(a: Int, b: Int)
  requires a <= b
  ensures a + a <= b + b {
}

pub trait Counted {
  fn count(self) -> Int

  fn twice(self) -> Int {
    mut i := 0
    mut s := 0
    while i < 2
      invariant i <= 2 {
      s += 1
      i += 1
    }
    s
  }

  law nonneg(x: Self) {
    n := x.count()
    assert n + n >= n + n by double_le(n, n)
    true
  }
}

capability Meter {
  fn read() -> Int {
    mut i := 0
    while i < 3
      invariant i <= 3 {
      i += 1
    }
    i
  }
}

test "loops" {
  mut i := 0
  while i < 3
    invariant i <= 3 {
    i += 1
  }
  assert i == 3
}
