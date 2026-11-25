from tools use run

test "uses a function the module lacks" {
  assert run(tests.input(["a"])) == 1
}
