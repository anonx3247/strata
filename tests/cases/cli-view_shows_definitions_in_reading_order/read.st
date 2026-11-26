from std.io use Console

type Expr =
  | Sum
  | Lit { value: Int }

/// A sum's two sides.
type Sum = { left: Int, right: Int, at: Span }

/// Where a sum is in its text.
type Span = { from: Int, to: Int }

fn main(console: Console) {
  match parse("1+2") {
    Some(e) => console.print(f"{shown(e)}"),
    None => console.print("no"),
  }
}

/// An expression, or `None`.
pub fn parse(s: String) -> Option[Expr] {
  match sum(s) {
    Some(e) => Some(e),
    None => literal(s),
  }
}

fn sum(s: String) -> Option[Expr] {
  match split(s, "+") {
    [a, b] => Some(Sum {
      left: number(a)?,
      right: number(b)?,
      at: Span { from: 0, to: byte_len(s) },
    }),
    _ => None,
  }
}

fn number(s: String) -> Option[Int] {
  digits(s)
}

fn digits(s: String) -> Option[Int] {
  if s == "" {
    None
  } else {
    Some(byte_len(s))
  }
}

fn literal(s: String) -> Option[Expr] {
  Some(Lit { value: number(s)? })
}

fn shown(e: Expr) -> String {
  if is_even(2) {
    "even"
  } else {
    "odd"
  }
}

fn is_even(n: Int) -> Bool {
  if n <= 0 {
    true
  } else {
    is_odd(n - 1)
  }
}

// even and odd call each other
fn is_odd(n: Int) -> Bool {
  if n <= 0 {
    false
  } else {
    is_even(n - 1)
  }
}

fn value_of(l: Lit) -> Int {
  l.value
}

test "a sum parses" {
  assert parse("1+2") == Some(Sum { left: 1, right: 2, at: Span { from: 0, to: 3 } })
}

--- exit 0
