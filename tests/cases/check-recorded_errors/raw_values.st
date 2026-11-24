fn peek(r: Raw) -> I64 {
  r.word(0)
}

fn main() {
  print(peek(Raw { address: 0 }))
}

fn fine() -> Int {
  1
}
