from std.io use Console

fn greet(mut c: Console, who: String) {
  mut c.print("hello " + who)
}

fn main(mut console: Console) {
  greet(mut console, "you")
  mut console.write("no newline")
  mut console.eprint("to stderr")
}
