from std.cli use CommandLine

/// Greet people.
type Options = {
  /// Shout.
  loud: Bool = false,
  /// Who.
  names: List[String] = [],
}

derive CommandLine for Options

fn main(opts: Options) {
  print(opts)
}
