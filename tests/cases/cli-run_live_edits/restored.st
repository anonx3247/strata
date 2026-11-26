/// What the server answers.
type Greeting = { text: String }

/// What the server answers now.
fn message() -> String {
  shout("back")
}

/// `s`, louder.
fn shout(s: String) -> String {
  s + "!"
}
