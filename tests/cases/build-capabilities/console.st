from std.io use Console

fn greet(mut console: Console, name: String) {
  mut console.print(f"hello, {name}")
}

fn main(mut console: Console) {
  greet(mut console, "world")
  mut console.eprint("to stderr")
  mut console.write("no newline")
  mut console.print("")
}
