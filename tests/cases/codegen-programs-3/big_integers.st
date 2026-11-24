fn factorial(n: Int) -> Int {
    mut acc := 1
    mut i := 2
    while i <= n {
        acc = acc * i
        i += 1
    }
    acc
}

fn main() {
    big := factorial(30)
    print(big)
    print(big / factorial(28))
    print(big - big + 1)
    print(big > factorial(29))
    print(big == factorial(30))
    print(4611686018427387904 - 1)
    print(-4611686018427387904 - 1)
    print(123456789012345678901234567890)
    print((1 << 70) | 1)
    print((1 << 70) & ((1 << 70) + 5))
    print((1 << 70) ^ 3)
}
