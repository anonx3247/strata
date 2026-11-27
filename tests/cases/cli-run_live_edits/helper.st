/// What the server answers now.
fn message() -> String {
  shout("again")
}

/// `s`, louder.
fn shout(s: String) -> String {
  s + "!"
}
