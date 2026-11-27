// The heap's live objects at their peak, by the place that made them: the
// records kept in a list, then a temporary list of their names. 20,000
// records of 32 bytes are 640,000 bytes; the names are 32-byte strings.

type Rec = { name: String, n: Int }

fn names(rs: List[Rec]) -> Int {
  mut out := []
  for r in rs {
    mut out.push(f"{r.name}!")
  }
  len(out)
}

fn main() {
  mut keep := []
  for i in 0..20000 {
    mut keep.push(Rec { name: f"rec {i}", n: i })
  }
  print(names(keep))
}
