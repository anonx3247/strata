from std.cli use CommandLine

type Options = {
  /// How many.
  count: Option[Int],
  names: List[String],
}

derive CommandLine for Options

fn main(mut opts: Options) {
  opts.names += ["end"]
  print(f"{opts.count} {opts.names}")
}
