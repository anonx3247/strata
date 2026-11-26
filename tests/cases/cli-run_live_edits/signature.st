/// `s`, louder.
fn shout(s: String, n: Int) -> String {
  s + "!" + to_string(n)
}

/// What the server answers now.
fn message() -> String {
  shout("again", 3)
}
