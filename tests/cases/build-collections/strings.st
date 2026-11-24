fn main() {
  cr := chr(13)
  text := "one two" + cr + "\nthree\n\nfour" + cr
  print(lines(text))
  print([lines(""), lines("\n"), lines("a\n"), lines("a\n\nb")])
  print(split("a,,b,", ","))
  print(split("", ","))
  print(split("héllo wörld", "ö"))
  print(trim(chr(160) + " \t x y " + chr(12288) + "\n"))
  print([
    contains("héllo", "ll"),
    contains("a", ""),
    starts_with("Zoë", "Zo"),
    ends_with("Zoë", "ë"),
    ends_with("a", "ab"),
  ])
  print(reverse("Zoë!"))
  print(chars("añb€😀"))
  print([
    get("Zoë".graphemes(), 2),
    get("Zoë".graphemes(), 3),
    get("Zoë".graphemes(), -1),
  ])
  print([
    join("héllo".graphemes()[1..3], ""),
    join(slice("héllo".graphemes(), -5, 2), ""),
    join(slice("héllo".graphemes(), 3, 1), ""),
    join(slice("héllo".graphemes(), 2, 100), ""),
  ])
  print([
    byte_slice("héllo", 0, 3),
    byte_slice("héllo", 3, 100),
    byte_slice("héllo", -1, 1),
    byte_len("héllo"),
  ])
  print(
    [
      index_of("añb€", "€"),
      index_of("abc", "z"),
      index_of("abc", ""),
    ],
  )
  print(
    replace("a-b-c--", "-", "+=") + "|" + replace("aaa", "aa", "b"),
  )
  print(repeat("ab", 3) + "|" + repeat("x", 0) + "|")
  print([
    to_upper("straße ǆ ŉ"),
    to_lower("ΟΔΟΣ Σ ΑΣ. İ"),
    to_upper("ß"),
    to_lower("ǅ"),
  ])
  print([
    is_digit("0123"),
    is_digit(""),
    is_digit("1a"),
    is_digit("٣"),
    is_alpha("añ日本"),
    is_alpha("a1"),
    is_space(" \t\n"),
    is_space(chr(8239)),
    is_space(""),
  ])
  print([ord("A"), ord("€"), ord("😀")])
  print([chr(65), chr(8364), chr(128512)])
  print([
    parse_int("42"),
    parse_int(" -7 "),
    parse_int("+3"),
    parse_int("1_000_"),
    parse_int("x"),
    parse_int("9223372036854775807"),
    parse_int("-9223372036854775808"),
  ])
  print([
    parse_float("2.5"),
    parse_float("-1e-3"),
    parse_float("5."),
    parse_float(".5"),
    parse_float("1e"),
    parse_float("inf"),
    parse_float("12"),
  ])
  x := 3.14159
  n := -42
  s := "héllo"
  print(
    f"[{x:.2}] [{x:8.3}] [{n:>6}] [{n:<6}] [{n:^7}] [{n:06}] [{n:+}] [{7:+05}] [{s:.2}] [{s:>8}] [{s:*^9}] [{s:3}] [{0.125:.2}] [{true:>6}] [{-0.0:.1}]",
  )
  w := "abc"
  print([
    w.graphemes()[0],
    w.graphemes()[2],
    join(w.graphemes()[1..], ""),
    join(w.graphemes()[..2], ""),
    join(w.graphemes()[..], ""),
  ])
  xs := [10, 20, 30, 40]
  print([xs[1], xs[3]])
  print([xs[1..3], xs[2..], xs[..1], xs[3..1], xs[-2..10]])
}
