type Sku = String where matches(it, "[a-z]{3}-[0-9]{2}")

type Code = String where matches(it, "[A-Z]+")

fn make_sku(s: String) -> Sku {
  if matches(s, "[a-z]{3}-[0-9]{2}") {
    s
  } else {
    "abc-00"
  }
}

fn bad_sku(s: String) -> Sku {
  if matches(s, "[a-z]+-[0-9]+") {
    s
  } else {
    "abc-00"
  }
}

fn upper(c: Code) -> Bool
  ensures it
{
  matches(c, "[A-Z]+")
}

fn not_both(s: String) -> Bool
  ensures !it
{
  matches(s, "[a-z]+") && matches(s, "[0-9]+")
}

fn letters(s: String) -> Bool
  ensures it ==> len(s.code_points()) > 0
{
  matches(s, "\\p{L}+")
}
