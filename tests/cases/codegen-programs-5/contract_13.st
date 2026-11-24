fn main() {
    mut k: Natural = 3
    k = k - read(5)
    print(k)
}

fn read(n: Int) -> Int {
    parse_int(f"{n}") ?? 0
}
