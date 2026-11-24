from std.cli use CommandLine
from std.io use Console

/// Greet people by name.
type Options = {
  /// Shout the greeting.
  loud: Bool = false,
  /// Say it this many times.
  times: Int where 1 <= it && it <= 10 = 1,
  /// Who to greet (the world if nobody).
  names: List[String] = [],
}

derive CommandLine for Options

fn main(opts: Options, mut console: Console) {
  who := if len(opts.names) == 0 {
    "world"
  } else {
    join(opts.names, " and ")
  }
  for _ in 0..opts.times {
    text := f"hello, {who}!"
    mut console.print(if opts.loud {
      to_upper(text)
    } else {
      text
    })
  }
  mut going := true
  while going {
    match mut console.read_line() {
      Some(l) => print("read " + l),
      None => {
        going = false
      },
    }
  }
}
