from std.io use Console, recorder

type Worker = { console: Console, count: Int }

fn work(mut w: Worker) {
  mut w.console.print(f"job {w.count}")
  w.count += 1
}

fn main(console: Console) {
  mut real := Worker { console, count: 1 }
  work(mut real)
  work(mut real)
  mut fake := Worker { console: recorder([]), count: 7 }
  work(mut fake)
  print(fake.count)
}
