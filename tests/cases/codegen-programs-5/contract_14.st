fn main() {
    mut i := 0
    while i < 5
        invariant i <= read(3)
    {
        i += 1
    }
}

fn read(n: Int) -> Int {
    parse_int(f"{n}") ?? 0
}
