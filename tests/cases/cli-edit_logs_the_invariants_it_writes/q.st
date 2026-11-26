type Lexer = { src: String, at: Int }

fn char_at(lx: Lexer, i: Int) -> String {
  if i >= 0 && i < byte_len(lx.src) {
    byte_slice(lx.src, i, i + 1)
  } else {
    ""
  }
}

fn quoted_end(src: String, from: Natural, limit: Int, tab: Int) -> Int
  requires from <= byte_len(src)
  ensures it <= byte_len(src)
{
  mut lx := Lexer { src, at: from }
  n := byte_len(src)
  mut s := ""
  mut col := 0
  while lx.at < n && char_at(lx, lx.at) != "\"" && col >= 0 {
    ch := char_at(lx, lx.at)
    if ch == "\\" && lx.at + 1 < n {
      lx.at += 1
      s += char_at(lx, lx.at)
    } else if ch == "\t" {
      col += tab
      s += ch
    } else {
      s += ch
    }
    col += 1
    lx.at += 1
  }
  lx.at
}

fn early(n: Int) -> Int
  ensures it >= 0 && it <= 10
{
  mut i: Int = 0
  while i < 10 {
    if i == n { break }
    i += 1
  }
  i
}
