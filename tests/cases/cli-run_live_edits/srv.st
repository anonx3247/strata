from std.io use Terminal
from std.time use SystemClock, ms

/// What the server answers.
type Greeting = { text: String }

/// What the server answers now.
fn message() -> String {
  Greeting { text: "hello" }.text
}

/// Prints `message()` each time it changes, until it is "stop".
fn main(mut term: Terminal, clock: SystemClock) {
  mut last := ""
  mut go := true
  while go {
    m := message()
    if m != last {
      mut term.print(m)
      last = m
    }
    if m == "stop" {
      go = false
    }
    clock.sleep(ms(10))
  }
}
