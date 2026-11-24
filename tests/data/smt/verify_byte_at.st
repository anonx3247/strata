// `byte_at(s, i)` faults unless 0 <= i < byte_len(s): a call that may be
// out of range is refuted, one inside the string is proven, and the byte
// is 0 to 255 (the character's code on an ASCII string).

fn any_offset(s: String, i: Int) -> Int {
  s.byte_at(i)
}

fn before_start(s: String) -> Int {
  byte_at(s, -1000)
}

/// As a first `cache.padded` did: `i` past the string's end, minus 1000.
fn padded_wrong(s: String, i: Int) -> Int {
  if i < byte_len(s) {
    0
  } else {
    byte_at(s, i - 1000)
  }
}

fn inside(s: String, i: Int) -> Int
  requires 0 <= i && i < s.byte_len()
  ensures 0 <= it && it <= 255
{
  s.byte_at(i)
}

fn slashes(path: String) -> Int {
  mut count := 0
  for k in 0..byte_len(path) {
    if byte_at(path, k) == 47 {
      count += 1
    }
  }
  count
}

fn ascii_code(s: String) -> Int
  requires s == "a"
  ensures it == 97
{
  byte_at(s, 0)
}
