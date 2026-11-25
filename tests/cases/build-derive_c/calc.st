from std.ffi use C

/// The sum of two numbers.
pub fn add(a: I32, b: I32) -> I32 {
  a + b
}

/// A greeting for `name`.
pub fn greet(name: String) -> String {
  "hello, " + name
}

/// Half of `x`.
pub fn half(x: Float) -> Float {
  x / 2.0
}

/// Whether `n` is even.
pub fn even(n: I64) -> Bool {
  n % 2 == 0
}

/// The next byte, wrapping.
pub fn next_byte(b: U8) -> U8 {
  b + 1
}

/// Says hello on standard output.
pub fn hello() {
  print("hello from Strata")
}

/// How many bytes `s` has.
pub fn size(s: String) -> I32 {
  byte_len(s).wrap_i32()
}

derive C for add

derive C for greet

derive C for half

derive C for even

derive C for next_byte

derive C for hello

derive C for size
