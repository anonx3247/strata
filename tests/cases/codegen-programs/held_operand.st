type Box = { label: String }

fn both(a: Box, n: Int) -> String {
    f"{a.label} {n}"
}

fn main() {
    mut b := Box { label: "first" + "!" }
    print(both(b, {
        b = Box { label: "second" }
        1
    }))
    print(b)
}
