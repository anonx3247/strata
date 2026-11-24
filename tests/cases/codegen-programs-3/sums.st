type Shape =
    | Circle { radius: Float }
    | Rect(Float, Float)
    | Dot

type Tree = Leaf | Node(Tree, Int, Tree)

fn area(s: Shape) -> Float {
    match s {
        Circle { radius } => 3.0 * radius * radius,
        Rect(w, h) => w * h,
        Dot => 0.0,
    }
}

fn insert(t: Tree, n: Int) -> Tree {
    match t {
        Leaf => Node(Leaf, n, Leaf),
        Node(l, m, r) => if n < m {
            Node(insert(l, n), m, r)
        } else {
            Node(l, m, insert(r, n))
        },
    }
}

fn total(t: Tree) -> Int {
    match t {
        Leaf => 0,
        Node(l, n, r) => total(l) + n + total(r),
    }
}

fn describe(n: Int) -> String {
    match n {
        0 => "zero",
        1 | 2 | 3 => "small",
        _ => "big",
    }
}

fn first_word(s: String) -> Option[Int] {
    if s == "" {
        None
    } else {
        Some(if s == "abc" { 3 } else { 5 })
    }
}

fn main() {
    print(area(Circle { radius: 1.0 }))
    print(area(Rect(2.0, 3.0)))
    print(area(Dot))
    print(Rect(2.0, 3.0))
    print(Dot)
    mut t := Leaf
    t = insert(t, 5)
    t = insert(t, 2)
    t = insert(t, 8)
    print(t)
    print(total(t))
    print(describe(0))
    print(describe(2))
    print(describe(9))
    print(first_word("abc"))
    print(first_word(""))
    o := first_word("hello")
    x := match o {
        Some(n) => n,
        None => -1,
    }
    print(x)
    print(o is Some)
    print(Dot is Dot)
    print(Rect(1.0, 2.0) == Rect(1.0, 2.0))
    print(Circle { radius: 1.0 } == Dot)
    let r: Result[Int, String] = Err("bad")
    match r {
        Ok(v) => print(v),
        Err(m) => print(f"error: {m}"),
    }
    print(r)
}
