type Shape =
  | Square { side: Positive }
  | Circle { radius: Positive }

type E =
  | A { x: Int }
  | B
  | C

type One = { n: Positive, a: Int }

type Two = { n: Positive }

type Holder = { s: Shape }

fn area(s: Shape) -> Positive {
  match s {
    Square => s.side * s.side,
    Circle => 3 * s.radius * s.radius,
  }
}

fn bad_area(s: Shape) -> Positive {
  match s {
    Square => s.side - 1,
    Circle => s.radius,
  }
}

fn pick(big: Bool) -> Shape {
  if big {
    Square { side: 5 }
  } else {
    Circle { radius: 1 }
  }
}

fn pick_area(big: Bool) -> Positive {
  area(pick(big))
}

fn make(big: Bool) -> Int where it > 2 {
  s := if big {
    Square { side: 5 }
  } else {
    Circle { radius: 1 }
  }
  match s {
    Square => s.side,
    Circle => s.radius + 2,
  }
}

fn bad_make(big: Bool) -> Int where it > 2 {
  s := if big {
    Square { side: 5 }
  } else {
    Circle { radius: 1 }
  }
  match s {
    Square => s.side,
    Circle => s.radius,
  }
}

fn describe(v: Int | String) -> Natural {
  match v {
    Int => abs(v),
    String => len(v.code_points()),
  }
}

fn bad_describe(v: Int | String) -> Positive {
  match v {
    Int => abs(v) + 1,
    String => len(v.code_points()),
  }
}

fn is_sq(s: Shape) -> Bool
  ensures it == (s is Square) {
  match s {
    Square => true,
    Circle => false,
  }
}

fn always_sq(s: Shape) -> Bool
  ensures it {
  s is Square
}

fn lit(v: Int | String) -> Positive {
  match v {
    0 => 1,
    "a" => 2,
    _ => 3,
  }
}

fn nonzero(x: Positive | Negative) -> Int where it != 0 {
  x
}

fn bad_nonzero(x: Int) -> Positive | Negative {
  x
}

fn variants(v: A | B) -> Natural {
  match v {
    A => abs(v.x),
    B => 0,
  }
}

fn bad_variants(v: A | C) -> Bool
  ensures it {
  v is A
}

fn opt(v: Option[Natural] | String) -> Natural {
  match v {
    Some(x) => x,
    None => 0,
    String => len(v.code_points()),
  }
}

fn bad_opt(v: Option[Int] | String) -> Natural {
  match v {
    Some(x) => x,
    None => 0,
    String => len(v.code_points()),
  }
}

fn self_eq(a: Int | String) -> Bool
  ensures it {
  a == a
}

fn bad_eq(a: Int | String, b: Int | String) -> Bool
  ensures it {
  a == b
}

fn bad_field(h: Holder) -> Bool
  ensures it {
  h.s is Circle
}

fn common(v: One | Two) -> Positive {
  v.n
}

fn bad_common(v: One | Two) -> Positive {
  v.n - 1
}

fn pos_test(n: Int) -> Bool
  ensures it == (n > 0) {
  n is Positive
}

fn bad_pos_test(n: Int) -> Bool
  ensures it {
  n is Positive
}

fn shape_or_int(big: Bool) -> Int where it > 0 {
  v := if big {
    3
  } else {
    "abc"
  }
  match v {
    Int => v,
    String => 1,
  }
}

fn kind(v: Int | String | Bool) -> Int where it >= 0 {
  match v {
    Int => 0,
    String => 1,
    Bool => 2,
  }
}
