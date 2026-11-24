type Small = Int where it < 10

fn f(x: Small) -> Small {
    x + 1
}

fn main() {
    print(f(read(3)))
    print(f(read(9)))
}

fn read(n: Int) -> Int {
    parse_int(f"{n}") ?? 0
}
