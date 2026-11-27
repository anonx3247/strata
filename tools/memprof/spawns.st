// 80 spawns, each capturing a list of 100,000 records that the main
// thread keeps: with copies, each spawn copies the list.


type Rec = { name: String, tags: List[String], n: Int }

fn total(rs: List[Rec]) -> Int {
  mut t := 0
  for r in rs {
    t += byte_len(r.name) + len(r.tags) + r.n
  }
  t
}

fn main() {
  mut rs := []
  for i in 0..100000 {
    mut rs.push(Rec { name: f"rec {i}", tags: [f"t{i % 7}", "x"], n: i })
  }
  mut sum := 0
  for _round in 0..10 {
    s := scope {
      mut hs := []
      for k in 0..8 {
        mut hs.push(spawn total(rs) + k)
      }
      await hs
    }
    for x in s {
      sum += x
    }
  }
  print(f"{sum}")
}
