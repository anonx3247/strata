type Point = { x: Int, y: Int }

type Status = Active | Closed { why: String }

fn close(s: Status) -> Status {
    match s {
        Active => Closed { why: "done" },
        Closed { .. } => s,
    }
}

fn main() -> Int {
    s := close(Active)
    code := match s {
        Active => 0,
        Closed { why: "done" } => 3,
        Closed { why } => 4,
    }
    print(code)
    assert close(s) == Closed { why: "done" }
    p := Point { x: 1, y: 2 }
    assert p == Point { ..p, y: 3 }
    code
}
