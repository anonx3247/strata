fn empty[T]() -> List[T] {
    []
}

fn nothing[T]() -> Option[T] {
    None
}

fn main() {
    let xs: List[String] = empty()
    print(push(xs, "a" + "b"))
    let o: Option[List[Int]] = nothing()
    print(o)
    mut ys: List[Int] = empty()
    ys = push(ys, 3)
    print(ys)
}
