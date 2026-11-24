fn main() {
  s := "héllo, wörld 👋🏽"
  print(s.bytes().len())
  print(s.code_points().len())
  print(len(s.code_points()))
  print(s.code_points().to_list())
  print([chr(c) for c in s.code_points() where c > 127].to_list())
  print(s.bytes().fold(0, (a, b) -> a + b))
  for b in "é".bytes() {
    print(b)
  }
  crlf := chr(13) + "\nab"
  print(crlf.graphemes().len())
  for w in ["éa", "👨‍👩‍👧x", "🇫🇷🇩🇪🇺", "한국어", "👋🏽!", "क्षि", ""] {
    g := w.graphemes()
    print(
      f"{g.len()} {[len(x.code_points().to_list()) for x in g].to_list()}",
    )
  }
}
