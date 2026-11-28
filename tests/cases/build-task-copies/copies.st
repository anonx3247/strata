// What crosses to a task is the task's own: a copy, or the value itself
// when its one holder gives it up. Changes on either side after the spawn
// (in place, as each holds its value once) don't reach the other side, and
// every object is freed once.

type Bag = { __label: String, items: List[Int] }

fn grow(b: Bag, n: Int) -> Bag {
  mut out := b
  for i in 0..n {
    mut out.items.push(i)
  }
  out
}

fn main() {
  mut xs := [1, 2, 3]
  mut b := Bag { __label: "bag", items: [10, 20] }
  (tx, rx) := channel(4)
  r := scope {
    // captures: copies, since `xs` and `b` stay the parent's
    seen := spawn (len(xs), len(b.items))
    mut xs.push(4)
    mut b.items.push(30)
    // the task changes its copy in place
    grown := spawn grow(b, 3)
    // a value held once moves: nothing to copy
    fresh := Bag { __label: "fresh", items: [7] }
    moved := spawn len(fresh.items)
    // a send copies what the sender keeps
    tx.send(b) except { () }
    mut b.items.push(40)
    got := spawn {
      mut n := 0
      for v in rx {
        n += len(v.items)
      }
      n
    }
    (await seen, len((await grown).items), await moved, await got)
  }
  print(f"{r} {xs} {b.items}")
}
