fn restock(mut n: Int, extra: Int) {
  n += extra
}

type Line = { sku: String, qty: Int }

fn bump(mut l: Line) {
  l.qty += 1
}

fn main() {
  mut k := 1
  restock(mut k, 5)
  print(k)
  mut line := Line { sku: "a", qty: 1 }
  bump(mut line)
  bump(mut line)
  print(line)
  restock(mut line.qty, 10)
  print(line)
}
