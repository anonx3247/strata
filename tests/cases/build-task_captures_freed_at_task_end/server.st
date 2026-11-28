// A long scope, as a server's: each task captures a copy of a big list
// (10,000 records) and is awaited before the next starts. A task's captures
// are freed when it ends, so the heap holds about one copy at a time, not
// one per finished task.

type Rec = { name: String, n: Int }

fn main() {
  mut big := []
  for i in 0..10000 {
    mut big.push(Rec { name: f"rec {i}", n: i })
  }
  total := scope {
    mut sum := 0
    for k in 0..100 {
      t := spawn len(big) + k
      sum += await t
      mut big.push(Rec { name: "one more", n: k })
    }
    sum
  }
  print(total)
}
