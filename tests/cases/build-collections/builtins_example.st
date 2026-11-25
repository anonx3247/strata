// A tour of the built-in types: Int, Float, Bool, String, () (unit, the
// "nothing" value), List, tuples, Dict, Set, Option and Result.

/// Split text on spaces into non-empty words.
fn words(text: String) -> List[NonEmptyString] {
  split(text, " ").map(trim).filter(w -> w != "")
}

/// How many times each word occurs.
fn frequencies(ws: List[NonEmptyString]) -> Dict[NonEmptyString, Int]
  ensures len(it) == len(set(ws))
  ensures all(ws, w -> contains(it, w)) {
  mut counts := Dict[NonEmptyString, Int] {}
  for w in ws {
    mut counts.insert(w, (get(counts, w) ?? 0) + 1)
  }
  counts
}

/// The `n` most frequent words, most frequent first (ties alphabetical).
fn top(
  freq: Dict[NonEmptyString, Int],
  n: Natural,
) -> List[(NonEmptyString, Int)]
  ensures len(it) <= n {
  take((items(freq)).sorted_by(pair -> (-pair.1, pair.0)), n)
}

/// Share of words that occur only once.
fn hapax_ratio(freq: Dict[NonEmptyString, Int]) -> Probability {
  if len(freq) == 0 {
    0.0
  } else {
    float(count(values(freq), c -> c == 1)) / float(len(freq))
  }
}

fn mean(xs: List[Float]) -> Option[Float] {
  match xs {
    [] => None,
    _ => Some(sum(xs) / float(len(xs))),
  }
}

type Stats = {
  distinct: Natural,
  top: List[(NonEmptyString, Positive)],
  hapax: Probability,
  mean_length: Option[Float],
}

fn stats(text: String) -> Stats {
  ws := words(text)
  freq := frequencies(ws)
  Stats {
    distinct: len(freq),
    top: top(freq, 3),
    hapax: hapax_ratio(freq),
    mean_length: mean(ws.map(w -> float(len(w.code_points())))),
  }
}

// Strings are UTF-8 and indexed by character.

/// "ada lovelace" -> "Ada Lovelace"
fn title_case(s: String) -> String {
  join(split(s, " ").map(
    w -> to_upper(join(w.graphemes()[..1], ""))
      + join(w.graphemes()[1..], ""),
  ), " ")
}

/// The digits of a string, as a number, if it has any.
fn digits(s: String) -> Option[Natural] {
  ds := s.chars().filter(is_digit)
  parse_int(join(ds, ""))
}

fn main() {
  {
    s := stats("the cat and the hat and the bat")
    assert s.distinct == 5
    assert s.top == [("the", 3), ("and", 2), ("bat", 1)]
    assert s.hapax == 0.6
    assert Set { 3, 1, 3, 2 } == Set { 1, 2, 3 }
    assert union(Set { "a" }, Set { "b" }) == Set { "b", "a" }
    (word, n) := match s.top {
      [first, ..] => first,
      [] => ("", 0),
    }
    assert word == "the" && n == 3
    assert (word, n).1 == 3
    assert (keys(Dict { "b": 2, "a": 1 })).sorted() == ["a", "b"]
    assert parse_int(" 42 ") == Some(42)
    assert round(2.6) == 3
  }
  {
    assert title_case("ada lovelace") == "Ada Lovelace"
    assert digits("tel: 06-12") == Some(612)
    assert digits("none") == None
    name := "Zoë"
    assert f"{name} has {len(name.graphemes())} letters" == "Zoë has 3 letters"
    assert name.graphemes().get(2) == Some("ë")
      && name.index_of("ë") == Some(2)
    assert "a-b-c".replace("-", "+") == "a+b+c"
  }
  print("all asserts hold")
}
