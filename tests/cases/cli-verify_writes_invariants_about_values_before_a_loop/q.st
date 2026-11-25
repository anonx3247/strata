fn char_at(src: String, i: Int) -> String {
  if i >= 0 && i < byte_len(src) {
    byte_slice(src, i, i + 1)
  } else {
    ""
  }
}

fn next(i: Int) -> Int
  ensures it == i + 1
{
  i + 1
}

fn quoted_end(src: String, from: Natural, tab: Int) -> Int
  requires from <= byte_len(src)
  ensures it <= byte_len(src)
{
  n := byte_len(src)
  mut at := from
  mut s := ""
  mut col := 0
  while at < n && char_at(src, at) != "\"" {
    ch := char_at(src, at)
    if ch == "\\" && at + 1 < n {
      at += 1
      s += char_at(src, at)
    } else if ch == "\t" {
      col += tab
      s += ch
    } else {
      s += ch
    }
    col += 1
    at = next(at)
  }
  at
}

fn start(src: String) -> Natural
  ensures it <= byte_len(src)
{
  if byte_len(src) > 2 { 1 } else { 0 }
}

fn quoted_after(src: String, tab: Int) -> Int
  ensures it <= byte_len(src)
{
  n := byte_len(src)
  mut at := start(src)
  mut s := ""
  mut start_at := 0
  while at < n && char_at(src, at) != "\"" {
    ch := char_at(src, at)
    if ch == "\\" && at + 1 < n {
      at += 1
      s += char_at(src, at)
    } else if ch == "\t" {
      start_at += tab
      s += ch
    } else {
      s += ch
    }
    start_at += 1
    at = next(at)
  }
  at
}
