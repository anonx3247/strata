type Item = { name: String, qty: Positive }

fn main() {
    print(Item { name: "a", qty: read(0) })
}

fn read(n: Int) -> Int {
    parse_int(f"{n}") ?? 0
}
