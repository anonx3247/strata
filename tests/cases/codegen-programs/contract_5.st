type Range = { lo: Int, hi: Int } where it.lo <= it.hi

fn main() {
    r := Range { lo: 1, hi: read(5) }
    print(r)
    q := Range { lo: 1, hi: read(0) }
    print(q)
}

fn read(n: Int) -> Int {
    parse_int(f"{n}") ?? 0
}
