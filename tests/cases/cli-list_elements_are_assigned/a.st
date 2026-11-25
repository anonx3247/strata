type Item = { name: String, qty: Int }

type Shop = { items: List[Item], counts: List[Int] }

fn bump(mut xs: List[Int], i: Int) {
  if 0 <= i && i < len(xs) {
    xs[i] += 10
  }
}

fn main() {
  mut xs := [1, 2, 3]
  xs[0] = 7
  xs[2] += 5
  print(xs)
  ys := xs
  xs[1] = 100
  print(xs)
  print(ys)
  mut grid := [[0, 0], [0, 0], [0, 0]]
  grid[1][0] = 5
  grid[2][1] = 9
  print(grid)
  mut s := Shop {
    items: [Item { name: "a", qty: 1 }, Item { name: "b", qty: 2 }],
    counts: [0, 0],
  }
  s.counts[1] = 3
  s.items[0].qty = 42
  s.items[1] = Item { name: "c", qty: 3 }
  print(s)
  bump(mut xs, 2)
  print(xs)
  mut names := ["x", "y"]
  k := 1
  names[k] = names[0] + "!"
  print(names)
  mut big := []
  for i in 0..10 {
    big = push(big, i)
  }
  for i in 0..len(big) {
    big[i] *= big[i]
  }
  print(big)
  zs := set_at(big, 0, -1)
  print(zs[0])
  print(big[0])
}
