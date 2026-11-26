/// What the server answers now.
fn message() -> String {
  to_string(doubled(600))
}

/// Twice `x`, said to stay below 1000.
fn doubled(x: Small) -> Small {
  x * 2
}

/// Small numbers.
type Small = Int where it >= 0 && it < 1000
