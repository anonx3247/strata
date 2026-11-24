from std.io use Console

fn main(mut console: Console) {
  mut going := true
  mut n := 0
  while going {
    match mut console.read_line() {
      Some(l) => {
        n += 1
        mut console.print(f"{n}: {l}")
      },
      None => {
        going = false
      },
    }
  }
  mut console.eprint("done")
}
