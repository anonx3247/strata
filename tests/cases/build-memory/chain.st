type Chain =
  | End
  | Link(Int, Chain)

fn main() {
  mut c: Chain = End
  mut i := 0
  while i < 1000000 {
    c = Link(i, c)
    i += 1
  }
  match c {
    Link(x, _) => print(x),
    End => print(0),
  }
}
