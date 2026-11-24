fn half(n: Int) -> Int
    requires n % 2 == 0
{
    n / 2
}

fn main() {
    print(half(4))
    print(half(read_odd()))
}

fn read_odd() -> Int {
    parse_int("3") ?? 0
}
