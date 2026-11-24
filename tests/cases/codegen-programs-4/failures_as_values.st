type Problem = | Empty | TooBig { max: Int }

fn parse(s: String) -> Result[Int, String] {
    match parse_int(s) {
        Some(n) => Ok(n),
        None => Err(f"not a number: {s}"),
    }
}

fn check(n: Int) -> Result[Int, Problem] {
    if n == 0 {
        return Err(Empty)
    }
    if n > 100 {
        return Err(TooBig { max: 100 })
    }
    Ok(n)
}

fn sum_two(a: String, b: String) -> Result[Int, String] {
    x := parse(a)!
    y := parse(b)!
    Ok(x + y)
}

fn first_even(xs: List[Int]) -> Option[Int] {
    x := find(xs, n -> n % 2 == 0)?
    Some(x * 10)
}

fn main() -> Result[(), String] {
    print(sum_two("2", "40"))
    print(sum_two("2", "x"))
    print(first_even([1, 3, 4]))
    print(first_even([1, 3]))
    print(parse("7") except { 0 })
    print(parse("q") except e { byte_len(e) })
    print(check(500) except e {
        match e {
            Empty => -1,
            TooBig { max } => max,
        }
    })
    print(get([1, 2], 5) ?? 99)
    print(get([1, 2], 1) ?? 99)
    print(parse_int("12") ?? 0)
    n := parse("8")!
    print(n)
    bad := parse("nope")!
    print(bad)
    Ok(())
}
