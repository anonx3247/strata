fn fib(n: Int) -> Int {
    if n < 2 {
        n
    } else {
        fib(n - 1) + fib(n - 2)
    }
}

fn main() {
    print(fib(20))
    print(7 / 2)
    print(-7 / 2)
    print(-7 % 3)
    print(-(3 * 4) + 1)
    print(6 & 3)
    print(6 | 3)
    print(6 ^ 3)
    print(1 << 10)
    print(1024 >> 3)
    print(2.5 * 4.0 - 1.0 / 4.0)
    print(7.5 % 2.0)
    print(-1.5)
    print(1 == 1 && 2 != 3)
    print(1 > 2 || 2 >= 2)
    print(false ==> 1 / 0 == 0)
    print(0.1 + 0.2 == 0.3)
    print(1.5 <= 1.5)
}
