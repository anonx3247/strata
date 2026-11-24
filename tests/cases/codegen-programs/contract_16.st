fn h(x: Int where it > 5) -> Int {
    x
}

fn main() {
    print(h(read(2)))
}

fn read(n: Int) -> Int {
    parse_int(f"{n}") ?? 0
}
