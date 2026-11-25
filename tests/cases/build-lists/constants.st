fn table() -> List[(Int, Int)] {
  [(1, 2), (3, 4)]
}

fn grow(mut xs: List[String]) {
  mut xs.push("d")
}

fn main() {
  mut xs := table()
  mut xs.push((5, 6))
  mut t := (1, 2)
  t.0 = 9
  mut zs := ["a", "b", "c"]
  grow(mut zs)
  print(
    f"{xs} {table()} {t} {(1, 2)} {zs} {["a", "b", "c"]} {[[1], [2, 3]]} {[None, Some(1)]}",
  )
}
