// A lambda with untyped parameters in a record's field, inside `Some`, in a
// generic function: its parameters' types come from the field's type (it
// was compiled without parameters and read garbage when called)

type Box[T] = { order: Option[(T, T) -> Int], name: String }

fn by[T](key: (T) -> Int) -> Box[T] {
  Box { order: Some((a, b) -> key(a) - key(b)), name: "by" }
}

fn main() {
  b := by((x: Int) -> x * 2)
  match b.order {
    Some(f) => print(f(5, 3)),
    None => print(0),
  }
  s := by((x: String) -> byte_len(x))
  match s.order {
    Some(f) => print(f("abc", "a")),
    None => print(0),
  }
}
