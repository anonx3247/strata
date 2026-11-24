use c

fn main() {
  print(c.numbers())
  print(c.texts())
  print(c.counting())
  print(c.refused())
}

test "null" {
  c.null_handle()
}

test "nul" {
  c.nul_string()
}
