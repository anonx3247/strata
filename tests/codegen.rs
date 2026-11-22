//! `compiler/codegen` (the compiler package's `codegen` command), the code
//! generator written in Strata: each program here is built by `strata
//! build` and by the Strata code generator (its IR linked with the same C
//! runtime by clang), and the two executables must print the same things and
//! end the same way (faults included). The IR itself may differ.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const STRATA: &str = env!("CARGO_BIN_EXE_strata");

#[path = "common/compiler.rs"]
mod compiler;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("strata-codegen-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn cc() -> String {
    std::env::var("STRATA_CC").unwrap_or_else(|_| "clang".into())
}

fn have_clang() -> bool {
    Command::new(cc()).arg("--version").output().is_ok_and(|o| o.status.success())
}

/// Store a program written as text at `file` (a module file).
fn save(file: &Path, text: &str) {
    let mut c = Command::new(STRATA).args(["--file", &file.to_string_lossy(), "edit"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success(), "{}: {}", file.display(), String::from_utf8_lossy(&o.stderr));
}

/// What an executable printed and how it ended.
fn outcome(exe: &Path) -> (String, String, Option<i32>) {
    let o = Command::new(exe).output().unwrap();
    (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned(), o.status.code())
}

/// The program at `file` built by Rust and by Strata: their outcomes.
fn both(dir: &Path, name: &str, file: &Path) -> ((String, String, Option<i32>), (String, String, Option<i32>)) {
    let rust = dir.join(format!("{name}-rust"));
    let o = Command::new(STRATA).arg("build").arg(file).arg("-o").arg(&rust).output().unwrap();
    assert!(o.status.success(), "{name}: strata build: {}", String::from_utf8_lossy(&o.stderr));
    let ir = compiler::run(&["codegen".to_string(), file.to_string_lossy().into_owned()]);
    assert!(!ir.starts_with("error: "), "{name}: not compiled by compiler/codegen:\n{ir}");
    let ll = dir.join(format!("{name}.ll"));
    std::fs::write(&ll, &ir).unwrap();
    let ours = dir.join(format!("{name}-strata"));
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/llvm/runtime.c");
    let o = Command::new(cc()).args(["-O2", "-Wno-override-module", "-o"]).arg(&ours).arg(&ll).arg(&runtime).arg("-lm").output().unwrap();
    assert!(o.status.success(), "{name}: clang refused compiler/codegen's IR: {}", String::from_utf8_lossy(&o.stderr));
    (outcome(&rust), outcome(&ours))
}

/// Programs of the constructs compiled so far (step 1: scalars, functions,
/// control flow; step 2: records, sums, tuples, `match`), each ending as it ends
/// (normally, with an exit code, or with a fault).
const PROGRAMS: &[(&str, &str)] = &[
    (
        "arithmetic",
        "fn fib(n: Int) -> Int {
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
",
    ),
    (
        "big_integers",
        "fn factorial(n: Int) -> Int {
    mut acc := 1
    mut i := 2
    while i <= n {
        acc = acc * i
        i += 1
    }
    acc
}

fn main() {
    big := factorial(30)
    print(big)
    print(big / factorial(28))
    print(big - big + 1)
    print(big > factorial(29))
    print(big == factorial(30))
    print(4611686018427387904 - 1)
    print(-4611686018427387904 - 1)
    print(123456789012345678901234567890)
    print((1 << 70) | 1)
    print((1 << 70) & ((1 << 70) + 5))
    print((1 << 70) ^ 3)
}
",
    ),
    (
        "strings",
        "fn greet(name: String) -> String {
    \"hello, \" + name
}

fn main() {
    s := greet(\"Ada\")
    print(s)
    t := \"hello, Ada\"
    print(f\"{s}! {1 + 2} {2.5} {true} {s == t}\")
    print(\"quote \\\" and backslash \\\\ and é\")
    print(\"\")
}
",
    ),
    (
        "loops",
        "fn first_square_over(n: Int) -> Int {
    mut i := 0
    loop {
        if i * i > n {
            return i
        }
        i += 1
    }
    0
}

fn main() {
    mut total := 0
    mut i := 0
    while i < 10 {
        i += 1
        if i % 2 == 0 {
            continue
        }
        if i > 7 {
            break
        }
        total += i
    }
    print(total)
    print(first_square_over(50))
    mut n := 0
    loop {
        n += 3
        if n > 10 {
            break
        }
    }
    print(n)
}
",
    ),
    (
        "exit_code",
        "fn main() -> Int {
    x := 40
    {
        x := 1
        print(x)
    }
    if x > 10 {
        return x + 2
    }
    0
}
",
    ),
    (
        "assert_fails",
        "fn check(a: Int, b: String) {
    assert b == \"b\"
    assert a + 1 < 3
}

fn main() {
    check(1, \"b\")
    print(\"one\")
    check(5, \"b\")
    print(\"never\")
}
",
    ),
    (
        "assert_strings",
        "fn main() {
    assert \"x\" + \"y\" == \"xz\"
}
",
    ),
    (
        "division_by_zero",
        "fn divide(a: Int, b: Int) -> Int {
    a / b
}

fn main() {
    print(divide(7, 2))
    print(divide(1, 0))
}
",
    ),
    (
        "float_overflow",
        "fn main() {
    mut x := 1.5
    while true {
        x = x * 10000000000.0
        print(x)
    }
}
",
    ),    (
        "records",
        "type Point = { x: Int, y: Int }

type Line = { from: Point, to: Point, label: String = \"line\" }

fn shift(p: Point, dx: Int) -> Point {
    Point { ..p, x: p.x + dx }
}

fn length2(l: Line) -> Int {
    dx := l.to.x - l.from.x
    dy := l.to.y - l.from.y
    dx * dx + dy * dy
}

fn main() {
    p := Point { x: 1, y: 2 }
    q := shift(p, 3)
    print(p)
    print(q)
    print(q.x + q.y)
    l := Line { from: p, to: q }
    print(l)
    print(length2(l))
    print(f\"{l.label}: {l.from} -> {l.to}\")
    print(p == Point { x: 1, y: 2 })
    print(p == q)
    print(p != q)
    pair := (p.x, \"two\", 3.5, true)
    print(pair)
    print(pair.1)
    (a, b, _, d) := pair
    print(f\"{a} {b} {d}\")
    print((1, (2, 3)) == (1, (2, 3)))
}
",
    ),
    (
        "sums",
        "type Shape =
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
        0 => \"zero\",
        1 | 2 | 3 => \"small\",
        _ => \"big\",
    }
}

fn first_word(s: String) -> Option[Int] {
    if s == \"\" {
        None
    } else {
        Some(if s == \"abc\" { 3 } else { 5 })
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
    print(first_word(\"abc\"))
    print(first_word(\"\"))
    o := first_word(\"hello\")
    x := match o {
        Some(n) => n,
        None => -1,
    }
    print(x)
    print(o is Some)
    print(Dot is Dot)
    print(Rect(1.0, 2.0) == Rect(1.0, 2.0))
    print(Circle { radius: 1.0 } == Dot)
    let r: Result[Int, String] = Err(\"bad\")
    match r {
        Ok(v) => print(v),
        Err(m) => print(f\"error: {m}\"),
    }
    print(r)
}
",
    ),
    (
        "generic",
        "type Pair[A, B] = { first: A, second: B }

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
        (0, _, _) => \"zero\",
        (_, true, \"yes\") => \"yes\",
        (n, false, s) => f\"{s}{n}\",
        _ => \"other\",
    }
}

fn divmod(a: Int, b: Int) -> (Int, Int) {
    (a / b, a % b)
}

fn main() {
    p := Pair { first: 1, second: \"one\" }
    print(p)
    print(swap(p))
    let nested: Option[Option[Int]] = Some(Some(4))
    print(nested)
    match nested {
        Some(Some(n)) => print(n),
        Some(None) => print(\"inner none\"),
        None => print(\"none\"),
    }
    e := Add(Mul { left: Num(1), right: Num(7) }, Neg(Add(Num(0), Num(2))))
    print(e)
    print(eval(e))
    print(classify((0, true, \"a\")))
    print(classify((1, true, \"yes\")))
    print(classify((2, false, \"n=\")))
    print(classify((3, true, \"no\")))
    (q, r) := divmod(17, 5)
    print(f\"{q} {r}\")
    ((a, b), c) := ((1, \"x\"), 2.5)
    print(f\"{a}{b}{c}\")
    print(Empty {})
    print(Empty {} == Empty {})
    fl := Flags { on: true, ratio: 0.5, note: \"say \\\"hi\\\"\\n\", nothing: () }
    print(fl)
    big := Pair { first: 1 << 80, second: true }
    print(big.first)
    w := match eval(e) as v {
        5 => f\"five {v}\",
        _ => f\"not five {v}\",
    }
    print(w)
    k := match 5 {
        where it > 3 => 3,
        _ => 0,
    }
    print(k)
}
",
    ),
    (
        "assert_records",
        "type Point = { x: Int, y: Int }

type Status = Active | Closed { why: String }

fn close(s: Status) -> Status {
    match s {
        Active => Closed { why: \"done\" },
        Closed { .. } => s,
    }
}

fn main() -> Int {
    s := close(Active)
    code := match s {
        Active => 0,
        Closed { why: \"done\" } => 3,
        Closed { why } => 4,
    }
    print(code)
    assert close(s) == Closed { why: \"done\" }
    p := Point { x: 1, y: 2 }
    assert p == Point { ..p, y: 3 }
    code
}
",
    ),
];

#[test]
fn compiled_programs_do_what_rust_compiled_ones_do() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let dir = scratch("programs");
    for (name, text) in PROGRAMS {
        let file = dir.join(format!("{name}.strata"));
        save(&file, text);
        let (rust, ours) = both(&dir, name, &file);
        assert_eq!(ours, rust, "{name}: (stdout, stderr, exit code) of compiler/codegen's build vs strata build's");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn refuses_what_it_does_not_compile_yet() {
    let dir = scratch("refused");
    let file = dir.join("lists.strata");
    save(&file, "fn main() {\n    xs := [1, 2]\n    print(xs)\n}\n");
    let out = compiler::run(&["codegen".to_string(), file.to_string_lossy().into_owned()]);
    assert!(out.starts_with("error: in `main`: not compiled yet: "), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}
