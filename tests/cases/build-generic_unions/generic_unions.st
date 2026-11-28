type TimedOut = { after: Int }

type Missing = { key: String }

type Attempt[E] = { last: E | TimedOut, tries: Int }

fn retry[T, E](n: Int, f: (Int) -> Result[T, E]) -> Result[T, E | TimedOut] {
  mut i := 0
  while i < n {
    match f(i) {
      Ok(v) => return Ok(v),
      Err(e) => if i == n - 1 {
        return Err(e)
      },
    }
    i = i + 1
  }
  Err(TimedOut { after: n })
}

fn gave_up[E](n: Int) -> Result[Int, E | TimedOut] = Err(TimedOut { after: n })

fn tens(i: Int) -> Result[Int, String] {
  if i < 2 {
    Err("too small")
  } else {
    Ok(i * 10)
  }
}

fn named(i: Int) -> Result[String, Missing] {
  if i < 1 {
    Err(Missing { key: "k" + to_string(i) })
  } else {
    Ok("found " + to_string(i))
  }
}

fn int_text(r: Result[Int, String | TimedOut]) -> String {
  match r {
    Ok(v) => f"ok {v}",
    Err(TimedOut { after }) => f"timed out after {after}",
    Err(String) => "failed",
  }
}

fn name_text(r: Result[String, Missing | TimedOut]) -> String {
  match r {
    Ok(v) => f"ok {v}",
    Err(TimedOut { after }) => f"timed out after {after}",
    Err(Missing { key }) => f"missing {key}",
  }
}

fn timed_out[E](v: E | TimedOut) -> Bool {
  match v {
    TimedOut => true,
    _ => false,
  }
}

fn choose[T, E](b: Bool, t: T, e: E) -> T | E {
  if b {
    t
  } else {
    e
  }
}

fn same[T, E](_t: T, _e: E, v: T | E) -> T | E = v

fn attempt[E](last: E | TimedOut, tries: Int) -> Attempt[E] = Attempt { last, tries }

fn maybe[E](b: Bool, e: E) -> Option[E | TimedOut] {
  if b {
    Some(e)
  } else {
    Some(TimedOut { after: 0 })
  }
}

fn picker[E](e: E) -> (Bool) -> E | TimedOut {
  b -> if b {
    e
  } else {
    TimedOut { after: 9 }
  }
}

fn kind(v: Int | String) -> String {
  match v {
    Int => f"int {v}",
    String => f"string {v}",
  }
}

fn main() {
  print(int_text(retry(3, tens)))
  print(int_text(retry(2, tens)))
  print(name_text(retry(3, named)))
  print(name_text(retry(1, named)))
  print(int_text(gave_up(4)))
  print(kind(choose(true, 4, "four")))
  print(kind(choose(false, 4, "four")))
  print(kind(same(0, "", choose(false, 5, "five"))))
  print(timed_out(TimedOut { after: 1 }))
  a := attempt(Missing { key: "a" }, 2)
  print(f"{timed_out(a.last)} after {a.tries}")
  b := attempt("text", 3)
  print(f"{timed_out(b.last)} after {b.tries}")
  match maybe(true, 3) {
    Some(Int) => print("some int"),
    Some(TimedOut) => print("some timeout"),
    None => print("none"),
  }
  match maybe(false, "x") {
    Some(String) => print("some string"),
    Some(TimedOut) => print("some timeout"),
    None => print("none"),
  }
  p := picker(Missing { key: "p" })
  for x in [true, false] {
    match p(x) {
      Missing { key } => print(f"picked missing {key}"),
      TimedOut { after } => print(f"picked timeout {after}"),
    }
  }
  q := picker(2)
  match q(true) {
    Int => print(f"picked int"),
    TimedOut => print("picked timeout"),
  }
}
