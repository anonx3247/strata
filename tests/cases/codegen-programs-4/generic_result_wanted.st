fn empty[T]() -> List[T] {
    []
}

fn nothing[T]() -> Option[T] {
    None
}

fn main() {
    let xs: List[String] = empty()
    print(xs + ["a" + "b"])
    let o: Option[List[Int]] = nothing()
    print(o)
    mut ys: List[Int] = empty()
    mut ys.push(3)
    print(ys)
}
