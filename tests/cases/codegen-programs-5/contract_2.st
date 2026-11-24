fn pos(n: Int) -> Int
    ensures it > 0
{
    n - 10
}

fn main() {
    print(pos(read(20)))
    print(pos(read(5)))
}

fn read(n: Int) -> Int {
    parse_int(f"{n}") ?? 0
}
