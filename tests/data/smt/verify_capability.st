from std.fs use Disk, Files

type Holder = { disk: Disk, n: Float }

type Outer = | Vacant | Full(Holder)

fn top(d: Disk, x: Float) -> Float
  ensures it >= 0.0 {
  x * x
}

fn opt(d: Option[Disk], x: Float) -> Float
  ensures it >= 0.0 {
  x * x
}

fn held(h: Holder) -> Float
  ensures it >= 0.0 {
  h.n * h.n
}

fn nested(o: List[Outer], x: Float) -> Float
  ensures it >= 0.0 {
  x * x
}

fn files(f: Files, x: Float) -> Float
  ensures it >= 0.0 {
  x * x
}

fn plain(x: Float) -> Float
  ensures it >= 0.0 {
  x * x
}
