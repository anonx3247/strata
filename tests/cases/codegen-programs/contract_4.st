fn g(n: Natural) -> Int {
    n
}

fn main() {
    print(g(read(3)))
    let k: Positive = read(0)
    print(k)
}

fn read(n: Int) -> Int {
    parse_int(f"{n}") ?? 0
}
