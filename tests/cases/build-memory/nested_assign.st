type P = { name: String, n: Int }

fn show2(a: String, b: Int) -> String {
  f"{a}/{b}"
}

fn pop(mut xs: List[String]) -> String {
  x := xs[len(xs) - 1]
  xs = xs[..len(xs) - 1]
  x
}

fn main() {
  mut s := f"first {1}"
  print(show2(s, {
    s = f"second {2}"
    1
  }))
  print(s)
  mut xs := [f"a{1}", f"b{2}"]
  print([xs, {
    xs = [f"c{3}"]
    []
  }])
  print(xs[0]
    + {
      xs = []
      "!"
    })
  mut p := P { name: f"p{1}", n: 1 }
  q := P { ..p, n: {
    p = P { name: f"z{9}", n: 9 }
    2
  } }
  print(q)
  mut ws := [f"w{1}", f"w{2}"]
  print((ws, (mut ws.pop())))
  print(ws)
}
