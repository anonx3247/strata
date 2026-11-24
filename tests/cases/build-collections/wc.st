// wc — count lines, words and characters, like the Unix tool.
//
//   strata run examples/wc -- README.md src/main.rs
//   strata run examples/wc -- --only words README.md
//   cat README.md | strata run examples/wc
//
// `main` asks for exactly what it touches: its options, the console and the
// disk, and hands them to `wc`, which takes any `Files`. The tests below hand
// `wc` stand-ins instead.

from std.cli use CommandLine

from std.io use Console, recorder, printed

from std.fs use Disk, Files, FsError, NotFound, mem_files

from std.process use ExitCode

/// What to count.
type Count =
  | Lines
  | Words
  | Chars

/// Count lines, words and characters in files, or in standard input.
type Options = {
  /// Show only this count.
  only: Option[Count],
  /// The files to count (standard input if none).
  files: List[String],
}

derive CommandLine for Options

type Counts = { lines: Natural, words: Natural, chars: Natural }

fn words(line: String) -> Natural {
  len(split(line, " ").filter(w -> trim(w) != ""))
}

fn count(text: String) -> Counts {
  ls := lines(text)
  Counts {
    lines: len(ls),
    words: sum(ls.map(words)),
    chars: len(text.code_points()),
  }
}

fn add(a: Counts, b: Counts) -> Counts {
  Counts {
    lines: a.lines + b.lines,
    words: a.words + b.words,
    chars: a.chars + b.chars,
  }
}

fn row(c: Counts, only: Option[Count], label: String) -> String {
  shown := match only {
    Some(Lines) => f"{c.lines}",
    Some(Words) => f"{c.words}",
    Some(Chars) => f"{c.chars}",
    None => f"{c.lines}\t{c.words}\t{c.chars}",
  }
  f"{shown}\t{label}"
}

/// All of standard input, line by line.
fn read_all(mut console: Console) -> String {
  mut text := ""
  loop {
    match mut console.read_line() {
      Some(line) => {
        text = text + line + "\n"
      },
      None => {
        break
      },
    }
  }
  text
}

/// What `main` does, with any console and any files.
fn wc(
  opts: Options,
  mut console: Console,
  files: Files,
) -> Result[ExitCode, FsError] {
  if len(opts.files) == 0 {
    mut console.print(row(count(read_all(mut console)), opts.only, "-"))
    return Ok(0)
  }
  mut total := Counts { lines: 0, words: 0, chars: 0 }
  for path in opts.files {
    c := count(files.read(path)!)
    mut console.print(row(c, opts.only, path))
    total = add(total, c)
  }
  if len(opts.files) > 1 {
    mut console.print(row(total, opts.only, "total"))
  }
  Ok(0)
}

fn main() {
  files := mem_files([
    ("a.txt", "one two\nthree\n"),
    ("b.txt", "four  five" + chr(13) + "\n"),
  ])
  for (only, names) in [
    (None, ["a.txt", "b.txt"]),
    (Some(Words), ["a.txt"]),
    (Some(Chars), ["b.txt", "a.txt"]),
    (None, ["nope.txt"]),
  ] {
    mut console := recorder([])
    r := wc(Options { only, files: names }, mut console, files)
    print(r)
    print(printed(console))
  }
  mut console := recorder(["hello world", "bye"])
  print(
    wc(
      Options { only: None, files: [] },
      mut console,
      mem_files([]),
    ),
  )
  print(printed(console))
  print(count("a b  c\n\nd"))
}
