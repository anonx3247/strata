type Pair[A, B] = { first: A, second: B }

fn twice[T](x: T, f: (T) -> T) -> T {
    f(f(x))
}

fn pair_of[A, B](a: A, b: B) -> Pair[A, B] {
    Pair { first: a, second: b }
}

fn first_or[T](xs: List[T], d: T) -> T {
    match xs {
        [x, ..] => x,
        [] => d,
    }
}

fn adder(n: Int) -> (Int) -> Int {
    x -> x + n
}

fn is_even(n: Int) -> Bool {
    n % 2 == 0
}

fn main() {
    print(twice(3, x -> x * 10))
    print(twice("ab", s -> s + s))
    print(pair_of(1, "one"))
    print(pair_of([1.5], Some(true)))
    print(first_or([7, 8], 0))
    print(first_or([], "none"))
    add5 := adder(5)
    print(add5(1))
    print(twice(1, adder(100)))
    xs := [1, 2, 3, 4, 5, 6]
    suffix := "!"
    print(map(xs, x -> x * x))
    print(xs.map(x -> f"{x}{suffix}"))
    print(filter(xs, is_even))
    print(xs.filter(x -> x > 3).map(x -> x - 3))
    print(fold(xs, 0, (acc, x) -> acc + x))
    print(all(xs, x -> x > 0))
    print(any(xs, x -> x > 5))
    print(find(xs, x -> x > 4))
    print(count(xs, is_even))
    print(contains(xs, 4))
    print(reverse(xs))
    print(join(["a", "b", "c"], "-"))
    print(zip(xs, ["a", "b"]))
    print(enumerate(["x", "y"]))
    print(sum(xs))
    print(sum([1.5, 2.25]))
    print([x * 2 for x in xs where x % 3 == 0])
    print([(a, b) for a in [1, 2] for b in ["p", "q"]])
    print([n for n in 0..4])
    names := [Pair { first: "z", second: 1 }, Pair { first: "y", second: 2 }]
    print(names.map(p -> p.first))
    mut total := 0
    for f in [adder(1), adder(2), x -> x * 3] {
        total += f(10)
    }
    print(total)
}
