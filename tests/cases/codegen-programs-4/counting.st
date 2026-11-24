type Node = { name: String, size: Int, kids: Tree }

type Tree = Empty | Branch(Node, Tree)

fn label(n: Int) -> String {
    if n % 3 == 0 {
        return f"fizz{n}"
    }
    "n" + f"{n}"
}

fn build(n: Int) -> Tree {
    mut t := Empty
    mut i := 0
    while i < n {
        i += 1
        if i == 4 {
            continue
        }
        t = Branch(Node { name: label(i), size: (1 << 70) + i, kids: Empty }, t)
        if i > 7 {
            break
        }
    }
    t
}

fn first_big(t: Tree) -> String {
    mut cur := t
    loop {
        match cur {
            Empty => return "none",
            Branch(n, rest) => {
                if n.size > (1 << 70) + 6 {
                    return n.name
                }
                cur = rest
            },
        }
    }
    "unreachable"
}

fn names(t: Tree) -> String {
    match t {
        Empty => "",
        Branch(Node { name, .. }, Empty) => name,
        Branch(n, rest) => n.name + "," + names(rest),
    }
}

fn pick(x: (String, Int)) -> String {
    match x {
        (s, 0) | (s, 1) => s + "!",
        (_, n) => f"{n}",
    }
}

fn main() {
    t := build(20)
    print(names(t))
    print(first_big(t))
    mut s := "a"
    mut k := 0
    while k < 5 {
        s = s + label(k)
        k += 1
    }
    print(s)
    print(pick(("x" + "y", 1)))
    print(pick(("x", 7)))
    big := { (1 << 80) * 3 }
    print(big)
    shadow := "one"
    shadow := shadow + "two"
    print(shadow)
    p := { n := Node { name: "inner" + "!", size: 1, kids: build(2) }
        n.kids }
    print(p)
    print(if names(p) == "n2,n1" { "yes" + "!" } else { "no" })
}
