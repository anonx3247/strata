fn total(xs: List[Natural]) -> Int {
    fold(xs, 0, (a, x) -> a + x)
}

fn main() {
    print(total([1, read(-2)]))
}

fn read(n: Int) -> Int {
    parse_int(f"{n}") ?? 0
}
