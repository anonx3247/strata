fn g(n: Natural) -> Int {
    n
}

fn main() {
    print(g(read(-1)))
}

fn read(n: Int) -> Int {
    parse_int(f"{n}") ?? 0
}
