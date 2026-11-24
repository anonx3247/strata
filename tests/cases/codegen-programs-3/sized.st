type Pixel = { r: U8, g: U8 }

fn rotr(x: U32, n: U32) -> U32 {
  x >> n | x << 32 - n
}

fn name(x: U8) -> String {
  match x {
    0 => "zero"
    255 => "max"
    _ => "other"
  }
}

fn main() {
  let a: U32 = 4294967295
  b := a + 1
  print(b)
  print(rotr(a, 7).to_int() + 5)
  print(rotr(2864434397, 13))
  let x: U8 = 200
  print(x + x)
  print(wrap_u8(300))
  print((-1).wrap_u32())
  let y: I8 = -128
  print(-y)
  print(y - 1)
  print(x / 7 + x % 7)
  let big: U64 = 18446744073709551615
  print(big)
  print(big + 1)
  print(big > 1)
  print(big.to_int() + 1)
  let m: I64 = -9223372036854775807
  print(m - 2)
  let n: I32 = -7
  print(n / 2)
  print(n % 2)
  print(n >> 1)
  print(n >> 40)
  let u: U32 = 1
  print(u << 31)
  print(u << 32)
  print(f"x={u + 1} {name(0)} {name(255)} {name(7)}")
  p := Pixel { r: 250, g: 10 }
  print(p)
  print(p.r + p.g)
  print(p == Pixel { r: 250, g: 10 })
  mut h: U32 = 0
  mut i := 0
  while i < 100 {
    h = h * 31 + 7
    i += 1
  }
  print(h)
}
