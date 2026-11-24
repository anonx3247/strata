from std.io use Console, recorder

type Worker = { console: Console, count: Int }

fn work(mut w: Worker) {
  mut w.console.print(f"job {w.count}")
}

fn main() {
  mut a := Worker { console: recorder([]), count: 1 }
  mut b := a
  work(mut a)
  work(mut a)
  work(mut b)
  print(a.console)
  print(b.console)
  print(a)
}
