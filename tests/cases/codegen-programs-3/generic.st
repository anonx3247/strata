type Pair[A, B] = { first: A, second: B }

type Expr =
    | Num(Int)
    | Add(Expr, Expr)
    | Mul { left: Expr, right: Expr }
    | Neg(Expr)

type Empty = {}

type Flags = { on: Bool, ratio: Float, note: String, nothing: () }

fn swap(p: Pair[Int, String]) -> Pair[String, Int] {
    Pair { first: p.second, second: p.first }
}

fn eval(e: Expr) -> Int {
    match e {
        Num(n) => n,
        Add(Num(0), r) => eval(r),
        Add(l, r) => eval(l) + eval(r),
        Mul { left: Num(1), right } => eval(right),
        Mul { left, right } => eval(left) * eval(right),
        Neg(x) => -eval(x),
    }
}

fn classify(t: (Int, Bool, String)) -> String {
    match t {
        (0, _, _) => "zero",
        (_, true, "yes") => "yes",
        (n, false, s) => f"{s}{n}",
        _ => "other",
    }
}

fn divmod(a: Int, b: Int) -> (Int, Int) {
    (a / b, a % b)
}

fn main() {
    p := Pair { first: 1, second: "one" }
    print(p)
    print(swap(p))
    let nested: Option[Option[Int]] = Some(Some(4))
    print(nested)
    match nested {
        Some(Some(n)) => print(n),
        Some(None) => print("inner none"),
        None => print("none"),
    }
    e := Add(Mul { left: Num(1), right: Num(7) }, Neg(Add(Num(0), Num(2))))
    print(e)
    print(eval(e))
    print(classify((0, true, "a")))
    print(classify((1, true, "yes")))
    print(classify((2, false, "n=")))
    print(classify((3, true, "no")))
    (q, r) := divmod(17, 5)
    print(f"{q} {r}")
    ((a, b), c) := ((1, "x"), 2.5)
    print(f"{a}{b}{c}")
    print(Empty {})
    print(Empty {} == Empty {})
    fl := Flags { on: true, ratio: 0.5, note: "say \"hi\"\n", nothing: () }
    print(fl)
    big := Pair { first: 1 << 80, second: true }
    print(big.first)
    w := match eval(e) as v {
        5 => f"five {v}",
        _ => f"not five {v}",
    }
    print(w)
    k := match 5 {
        where it > 3 => 3,
        _ => 0,
    }
    print(k)
}
