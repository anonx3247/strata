from std.lemmas use frame_with, frame_grows

/// Sets every key of `ks` to 0; nothing else changes.
pub fn zeroed(d: Dict[Int, Int], ks: List[Int]) -> Dict[Int, Int]
  ensures it.excluding(set(ks)) == d.excluding(set(ks)) {
  mut out := d
  mut done := Set[Int] {}
  mut i := 0
  while i < len(ks)
    invariant i >= 0
    invariant out.excluding(done) == d.excluding(done)
    invariant done == set(ks[..i]) {
    k := ks[i]
    assert out.with(k, 0).excluding(done.with(k)) == d.excluding(done.with(k)) by {
      frame_with(out, k, 0, done)
      frame_grows(out, d, done, k)
    }
    out = out.with(k, 0)
    done = done.with(k)
    i += 1
  }
  out
}

fn inc(x: Int) -> Int
  ensures it == x + 1 {
  x + 1
}

lemma double_le(a: Int, b: Int)
  requires a <= b
  ensures a + a <= b + b

/// The function's facts (`inc`'s `ensures`) are known to a plain `assert`.
pub fn plain(x: Int) -> Int {
  y := inc(x)
  assert y > x
  y
}

/// An `assert … by` sees only its steps' facts (and the path's
/// conditions): `inc`'s `ensures` isn't among them.
pub fn alone(x: Int) -> Int {
  y := inc(x)
  assert y > x by {}
  y
}

/// A lemma's `requires` is checked where it is called, its `ensures` is
/// the one fact the proof gives; a `calc` proves each step alone.
pub fn chain(x: Int, y: Int) -> Int
  requires x <= y {
  assert x + x <= y + y by double_le(x, y)
  calc {
    x + x <= y + y
    _ == 2 * y
  }
  x + x
}

/// A step that doesn't hold.
pub fn bad_step(x: Int) -> Int {
  calc {
    x + 1 == 1 + x
    _ <= x
  }
  x
}

/// A lemma called outside its `requires`.
pub fn outside(x: Int, y: Int) -> Int {
  assert x + x <= y + y by double_le(x, y)
  x
}

fn main() {
  z := zeroed(Dict[Int, Int] { 1: 5, 2: 6 }, [1])
  print(f"{get(z, 1) ?? -1} {get(z, 2) ?? -1}")
}
