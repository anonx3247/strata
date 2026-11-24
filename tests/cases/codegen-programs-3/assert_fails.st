fn check(a: Int, b: String) {
    assert b == "b"
    assert a + 1 < 3
}

fn main() {
    check(1, "b")
    print("one")
    check(5, "b")
    print("never")
}
