type Inner = { y: Int, tag: String }

type Outer = { x: Int, inner: Inner, items: List[String] }

type Counter = { n: Int, log: List[String] }

impl Counter {
    fn bump(mut self, by: Int) {
        self.n += by
        mut self.log.push(f"+{by}")
    }

    fn total(self) -> Int {
        self.n
    }
}

fn push_twice(mut xs: List[Int], x: Int) {
    mut xs.push(x)
    mut xs.push(x * 10)
}

fn rename(mut o: Outer, name: String) {
    o.inner.tag = name
}

fn swap(mut a: String, mut b: String) {
    t := a
    a = b
    b = t
}

fn main() {
    mut xs := [1]
    push_twice(mut xs, 2)
    print(xs)
    mut o := Outer { x: 1, inner: Inner { y: 2, tag: "a" }, items: [] }
    keep := o
    o.x = 10
    o.inner.y = 20
    mut o.items.push("first" + "!")
    print(o)
    print(keep)
    rename(mut o, "renamed")
    print(o.inner)
    mut c := Counter { n: 0, log: [] }
    mut c.bump(3)
    mut c.bump(4)
    print(c.total())
    print(c)
    mut p := "left"
    mut q := "right"
    swap(mut p, mut q)
    print(f"{p} {q}")
    mut pair := (1, "one")
    pair.0 = 2
    print(pair)
}
