from m use Worker

fn main(w: Worker) {
  for line in w.all() {
    print(line)
  }
  print(w.many())
}

test "a test asks for the capability" (w: Worker) {
  assert w.inc(1) == Ok(2)
}
