// Tasks pushing onto one list from many threads at once: each push past
// the shared buffer's end takes its slot alone (core.list's slot_taken)

fn hammer(base: List[Int], id: Int) -> Int {
  mut bad := 0
  for i in 0..20000 {
    ys := push(base, id * 100000 + i)
    zs := push(ys, id)
    if get(ys, len(base)) != Some(id * 100000 + i) || get(zs, len(base) + 1) != Some(id) {
      bad += 1
    }
  }
  bad
}

fn main() {
  mut base := []
  for i in 0..5 {
    base = push(base, i)
  }
  bad := scope {
    mut hs := []
    for id in 0..12 {
      hs = push(hs, spawn hammer(base, id))
    }
    sum(await hs)
  }
  print(bad)
  print(base)
}
