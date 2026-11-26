/// What the server answers.
type Greeting = { text: String, loud: Bool }

/// What the server answers now.
fn message() -> String {
  Greeting { text: "hello", loud: false }.text
}
