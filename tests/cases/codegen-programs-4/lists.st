type Item = { name: String, qty: Int }

fn total(items: List[Item]) -> Int {
    mut sum := 0
    for it in items {
        sum += it.qty
    }
    sum
}

fn squares(n: Int) -> List[Int] {
    mut out := []
    for i in 0..n {
        mut out.push(i * i)
    }
    out
}

fn describe(xs: List[Int]) -> String {
    match xs {
        [] => "empty",
        [x] => f"one: {x}",
        [a, b, ..rest] => f"{a}, {b} and {len(rest)} more",
    }
}

fn main() {
    xs := [3, 1, 4, 1, 5, 9, 2, 6]
    print(xs)
    print(len(xs))
    print(xs[2])
    print(xs[2..5])
    print(xs[..3])
    print(xs[6..])
    print(xs[1..=2])
    print(xs.len())
    print(get(xs, 1))
    print(get(xs, 10))
    print(take(xs, 2))
    print(drop(xs, 6))
    print(xs + [7, 8])
    print(squares(6))
    print(describe([]))
    print(describe([42]))
    print(describe(xs))
    items := [Item { name: "tea", qty: 2 }, Item { name: "cake", qty: 3 }]
    print(items)
    print(total(items))
    names := ["a" + "b", "c"]
    print(names)
    print(names == ["ab", "c"])
    print([[1, 2], [3]] == [[1, 2], [3]])
    print([1, 2] == [1, 3])
    print([(1, "x"), (2, "y")])
    mut acc := ""
    for (n, s) in [(1, "x"), (2, "y")] {
        if n == 2 {
            break
        }
        acc = acc + s
    }
    print(acc)
    mut count := 0
    for i in 1..=10 {
        if i % 2 == 0 {
            continue
        }
        count += i
    }
    print(count)
    big := [1 << 70, 2]
    print(big)
    print(range(2, 5))
    let ys: List[String] = []
    print(ys + ["z"])
}
