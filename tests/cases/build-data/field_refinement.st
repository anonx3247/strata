type Quantity = Int
  where 1 <= it && it <= 99

type Line = { sku: String, qty: Quantity }

fn main() {
  mut l := Line { sku: "a", qty: 5 }
  l.qty = 0
}
