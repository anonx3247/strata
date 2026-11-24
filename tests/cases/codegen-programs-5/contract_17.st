distinct type Age = Int where it >= 0

fn main() {
    print(Age(read(4)))
    print(Age(read(-4)))
}

fn read(n: Int) -> Int {
    parse_int(f"{n}") ?? 0
}
