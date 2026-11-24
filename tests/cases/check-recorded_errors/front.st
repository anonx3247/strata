from std.time use SystemClock, ms

type Name = String

type Line = { qty: Int }

type Shape =
  | Square { side: Int }
  | Circle { radius: Int }

trait Priced {
  fn price(self) -> Int
}

trait Counter {
  fn tick(mut self)
  fn peek(self) -> Int
}

impl Counter for Line {
  fn tick(mut self) {
    self.qty += 1
  }

  fn peek(self) -> Int {
    self.qty
  }
}

impl Line {
  // (inside a method too)
  fn twice(mut self) {
    self.tick()
    self.tick()
  }
}

extern c type Handle

extern c fn abs(x: Int) -> List[Int]

extern c fn fill(mut s: String, h: Handle) -> I32

type Box = { n: Int }

impl Box {
  fn bump(mut self) {
    self.n += 1
  }
}

fn own_unmarked() {
  mut b := Box { n: 1 }
  b.bump()
}

fn over_record(l: Line) {
  for x in l {
    print(x)
  }
}

fn over_string(s: String) {
  for c in s {
    print(c)
  }
}

fn over_param[I](it: I) {
  for x in it {
    print(x)
  }
}

fn nobound[T](xs: List[T]) -> Int {
  sum(xs.map(price))
}

fn notimpl() -> Int {
  true.price()
}

fn unmarked() {
  mut l := Line { qty: 1 }
  l.tick()
}

fn needless() -> Int {
  mut l := Line { qty: 1 }
  mut l.peek()
}

fn builtin() -> Int {
  mut xs := [1]
  len(mut xs)
}

fn grow(mut shapes: List[Shape]) {
  shapes += [Square { side: 1 }]
}

fn exact() -> Int {
  mut cs := [Circle { radius: 1 }]
  grow(mut cs)
  len(cs)
}

fn not_a_clock(n: Int, rx: Receiver[Int]) -> Int {
  select {
    rx.recv() as x => 1,
    n.after(ms(5)) => 2,
  }
}

fn plain(clock: SystemClock, rx: Receiver[Int]) -> Int {
  select {
    rx.recv() as x => 1,
    clock.after(5) => 2,
  }
}

fn forge() -> Handle {
  Handle(1)
}

fn main() {
  print(1)
}
