type Chain =
  | Link(Int, Chain)
  | End

fn down(n: Int) -> Int {
  if n == 0 {
    0
  } else {
    1 + down(n - 1)
  }
}

fn length(c: Chain) -> Int {
  match c {
    Link(_, rest) => 1 + length(rest),
    End => 0,
  }
}

fn main() {
  print(down(20000))
  mut c: Chain = End
  mut i: Int = 0
  while i < 1500000 {
    i += 1
  }
  i = 0
  while i < 20000 {
    c = Link(i, c)
    i += 1
  }
  print(length(c))
  print(down(1000000000))
}

test "runaway" {
  assert down(100) == 100
  down(-1)
}
