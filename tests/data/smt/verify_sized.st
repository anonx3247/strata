fn add_u8(a: U8, b: U8) -> U8 {
  a + b
}

fn widen(a: U8) -> Int
  ensures 0 <= it && it <= 255 {
  a.to_int()
}

fn flip(a: I8) -> I8 {
  -a
}

fn narrow(n: Int) -> U8
  requires 0 <= n && n <= 255 {
  n.to_u8()
}

fn bad_narrow(n: Int) -> U8 {
  n.to_u8()
}

fn wrapped(n: Int) -> Int
  ensures 0 <= it && it < 256 {
  n.wrap_u8().to_int()
}

fn sum32(a: U32, b: U32) -> Natural {
  (a + b).to_int()
}

fn wrong_sum(a: U8, b: U8) -> Int
  ensures it == a.to_int() + b.to_int() {
  (a + b).to_int()
}

fn signed_wrap(a: I8) -> Int
  ensures it >= -128 {
  (a - 1.to_i8()).to_int()
}

fn compare(a: U16, b: U16) -> Bool
  ensures it == (a.to_int() < b.to_int()) {
  a < b
}
