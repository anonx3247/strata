type P = { name: String, n: Int, x: Float, on: Bool }

type Shape =
  | Dot
  | Circle(Float)
  | Named { label: String, sides: Int }

fn main() {
  p := P { name: "a", n: 1, x: 1.5, on: true }
  mut q := P { name: "a", n: 1, x: 1.5, on: true }
  q.n = 2
  print(
    f"{p} {q} {Some(1)} {[Some(2), None]} {Circle(2.5)} {Named { label: "sq", sides: 4 }} {Ok(1) == Ok(1)}",
  )
}
