fn rotr(x: U32, n: U32) -> U32 {
  x >> n | x << 32 - n
}

fn ok() -> U32 {
  let a: U32 = 4294967295
  let m: I64 = -9223372036854775808
  rotr(a, 7) + a * 3 - 1
}

fn bad() {
  let a: U32 = 5
  n := 3
  print(a + n)
  print(a << n)
  let b: U8 = 300
  let c: U32 = -1
  let d: I8 = 5
  print(a == d)
  print(-n + -a)
  let e: U32 = n
}
