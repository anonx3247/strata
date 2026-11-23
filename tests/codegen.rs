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

/// What the runtime's memory check (`STRATA_MEMORY=check`: a freed object
/// used aborts) says at the end of a run: `memory: N allocated, N freed, 0 live`.
fn memory(exe: &Path) -> String {
    let o = Command::new(exe).env("STRATA_MEMORY", "check").output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr).into_owned();
    assert!(o.status.success() && !err.contains("of a freed object"), "{}: {err}", exe.display());
    err.lines().find(|l| l.starts_with("memory: ")).unwrap_or_default().to_string()
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
    let (r, o) = (outcome(&rust), outcome(&ours));
    // a program that ends normally frees everything it made
    if r.2 == Some(0) {
        let m = memory(&ours);
        assert!(m.ends_with(" 0 live"), "{name}: compiler/codegen's build leaks: {m}");
    }
    (r, o)
}

/// Programs of the constructs compiled so far (step 1: scalars, functions,
/// control flow; step 2: records, sums, tuples, `match`; step 3: counting; then group A of the plan: lists, generic functions, closures, the prelude, core's built-ins, sets and dicts, format specs), each ending as it ends
/// (normally, with an exit code, or with a fault).
const PROGRAMS: &[(&str, &str)] = &[
    ("console", "from std.io use Console

fn greet(mut c: Console, who: String) {
  mut c.print(\"hello \" + who)
}

fn main(mut console: Console) {
  greet(mut console, \"you\")
  mut console.write(\"no newline\")
  mut console.eprint(\"to stderr\")
}
"),
    ("recorder", "from std.io use recorder, printed

fn main() {
  r := recorder([\"a\"])
  print(printed(r))
}
"),
    ("own_capability", "capability Db {}

fn main(db: Db) {
  print(1)
}
"),
    ("disk_and_process", "from std.fs use Disk, Files, FsError
from std.process use Process, Environment, ExitCode

fn copy(mut f: Files, from: String, to: String) -> Result[Int, FsError] {
  text := f.read(from)!
  mut f.write(to, text + \"!\")!
  Ok(len(text.bytes()))
}

fn main(mut disk: Disk, process: Process) -> Result[ExitCode, String] {
  dir := \"/tmp/strata-codegen-c4\"
  mut disk.make_dir(dir) except e {
    return Err(f\"no dir: {e}\")
  }
  mut disk.write(dir + \"/a.txt\", \"hello\") except e {
    return Err(f\"{e}\")
  }
  print(copy(mut disk, dir + \"/a.txt\", dir + \"/b.txt\"))
  print(disk.read(dir + \"/b.txt\"))
  print(disk.read(dir + \"/missing.txt\"))
  print(disk.exists(dir + \"/b.txt\"))
  print(disk.list(dir))
  print(disk.read_bytes(dir + \"/a.txt\"))
  mut disk.remove(dir + \"/a.txt\") except e {
    return Err(f\"{e}\")
  }
  mut disk.remove(dir + \"/b.txt\") except e {
    return Err(f\"{e}\")
  }
  print(disk.list(dir))
  print(process.args())
  print(process.var(\"STRATA_SURELY_UNSET\"))
  print(process.var(\"HOME\") is Some)
  Ok(3)
}
"),
    ("console_input", "from std.io use Console

fn main(mut console: Console) {
  mut going := true
  mut n := 0
  while going {
    match mut console.read_line() {
      Some(l) => {
        n += 1
        mut console.print(f\"{n}: {l}\")
      },
      None => {
        going = false
      },
    }
  }
  mut console.eprint(\"done\")
}
"),
    ("json_and_regex", "from std.json use Json, parse, stringify
from std.regex use full_match

fn main() {
  match parse(\"{\\\"a\\\": [1, 2.5, true, null, \\\"s\\\"], \\\"b\\\": {}}\") {
    Ok(j) => print(stringify(j)),
    Err(e) => print(e),
  }
  print(parse(\"[1,\"))
  print(full_match(\"ab1\", \"[a-z]+[0-9]\"))
  print(full_match(\"ab\", \"[a-z]+[0-9]\"))
  print(full_match(\"x\", \"(\"))
}
"),
    ("launcher_and_json", "from std.process use Launcher, Commands
from std.json use Json

type Point = { x: Int, y: Int }

derive Json for Point

fn main(mut launcher: Launcher) {
  print(mut launcher.run(\"echo\", [\"hi\", \"there\"], \"\"))
  print(mut launcher.run(\"no-such-program-x\", [], \"\"))
  p := Point { x: 1, y: 2 }
  print(p.to_json())
}
"),
    ("tasks_and_channels", "fn main() {
  (tx, rx) := channel()
  results := scope {
    a := spawn {
      tx.send(1)
      tx.send(2)
      tx.close()
      10
    }
    b := spawn 32
    tx.close()
    mut got := []
    for v in rx {
      got = push(got, v)
    }
    (await a + await b, got)
  }
  print(results)
}
"),
    ("channel_in_scope", "fn main() {
  let (tx, rx): (Sender[Int], Receiver[Int]) = channel()
  r := scope {
    a := spawn 1
    tx.close()
    mut got := []
    for v in rx {
      got = push(got, v)
    }
    got
  }
  print(r)
}
"),
    ("select_race_supervised", "from std.time use SystemClock, VirtualClock, Clock, seconds, ms

fn slow(c: VirtualClock, n: Int) -> Int {
  c.sleep(seconds(n))
  n
}

fn answer(questions: Receiver[Int]) -> Int {
  questions.recv() ?? 0
}

fn risky(n: Int) -> Int {
  100 / n
}

fn main(clock: VirtualClock) {
  (tx, rx) := channel(1)
  (done_tx, done_rx) := channel()
  r := scope {
    h := spawn {
      tx.send(41)
      tx.close()
      done_tx.close()
      7
    }
    done_tx.close()
    tx.close()
    mut total := 0
    mut going := true
    while going {
      select {
        rx.recv() as v => match v {
          Some(x) => {
            total += x
          },
          None => {
            going = false
          },
        },
        clock.after(seconds(5)) => {
          going = false
        },
      }
    }
    for _ in done_rx {
    }
    total + await h
  }
  print(r)
  (ask, questions) := channel()
  w := race {
    answer(questions) as v => Some(v),
    clock.sleep(seconds(30)) => None,
  }
  ask.close()
  print(w)
  print(clock.now().value)
  z := race {
    slow(clock, 3) as a => a,
    slow(clock, 1) as b => b * 10,
  }
  print(z)
  safe := scope {
    good := supervised spawn risky(5)
    bad := supervised spawn risky(0)
    (await good, await bad)
  }
  print(safe.0)
  match safe.1 {
    Ok(x) => print(x),
    Err(p) => print(p.kind + \": \" + p.message),
  }
  hs := scope {
    tasks := [spawn 1, spawn 2, spawn 3]
    await tasks
  }
  print(hs)
}
"),
    ("cancelling_and_timers", "from std.time use VirtualClock, Clock, seconds, Instant, Duration

fn deep(c: VirtualClock, n: Int, held: String) -> String {
  if n == 0 {
    c.sleep(seconds(100))
    held + \"!\"
  } else {
    inner := held + to_string(n)
    deep(c, n - 1, inner)
  }
}

fn wait_on(clock: Clock) -> String {
  (tx, rx) := channel()
  tx.close()
  select {
    clock.after(seconds(1)) => \"timer\",
    rx.recv() as v => match v {
      Some(x) => x,
      None => \"closed\",
    },
  }
}

fn main(clock: VirtualClock) {
  w := race {
    deep(clock, 5, \"start\") as s => s,
    clock.sleep(seconds(1)) => \"timeout\",
  }
  print(w)
  (tx, rx) := channel()
  got := scope {
    spawn {
      for i in 0..3 {
        tx.send(\"item \" + to_string(i))
      }
      tx.close()
    }
    tx.close()
    mut out := []
    for v in rx {
      out = push(out, v)
    }
    out
  }
  print(got)
  (out, inp) := channel(1)
  r := select {
    out.send(\"x\" + \"y\") => \"sent\",
    clock.after(seconds(2)) => \"late\",
  }
  out.close()
  print(r)
  print(inp.recv())
  pair := scope {
    a := spawn \"a\" + \"b\"
    b := spawn 2
    await (a, b)
  }
  print(pair)
  print(wait_on(clock))
}
"),
    ("panic_in_a_task", "fn work(n: Int) -> Int {
  if n == 3 {
    [1, 2][5]
  } else {
    n
  }
}

fn main() {
  r := scope {
    a := spawn work(1)
    b := spawn work(3)
    await a + await b
  }
  print(r)
}
"),
    ("options_without_numbers", "from std.cli use CommandLine

/// Greet people.
type Options = {
  /// Shout.
  loud: Bool = false,
  /// Who.
  names: List[String] = [],
}

derive CommandLine for Options

fn main(opts: Options) {
  print(opts)
}
"),
    ("iterators", "type Naturals = { n: Int }

impl Iterator[Int] for Naturals {
  fn next(mut self) -> Option[Int] {
    self.n += 1
    Some(self.n - 1)
  }
}

fn total[I: FiniteIterator[Int]](it: I) -> Int {
  it.fold(0, (a, x) -> a + x)
}

fn firsts[I: Iterator[Int]](it: I, n: Int) -> List[Int] {
  mut out := []
  for x in it.take(n) {
    out = push(out, x)
  }
  out
}

fn main() {
  xs := [5, 6, 7, 8, 9]
  print(xs.iter().drop(2).to_list())
  print(xs.iter().filter(x -> x % 2 == 0).len())
  print(total(xs.iter().map(x -> x * 2)))
  print(xs.iter().get(3))
  print(xs.iter().get(9))
  it := xs.iter()
  print(it[1])
  print(it[1..3].to_list())
  print(firsts(Naturals { n: 10 }, 4))
  print(Naturals { n: 0 }.filter(x -> x % 3 == 0).take(3).to_list())
  s := Set { 3, 1, 2 }
  for x in s.iter() {
    print(x)
  }
  d := Dict { \"a\": 1, \"b\": 2 }
  for (k, v) in d.iter() {
    print(f\"{k}={v}\")
  }
  print([x + 1 for x in xs.iter().take(2)])
  print(join([\"a\", \"b\", \"c\"].iter().map(s -> s + \"!\"), \", \"))
  print(set(xs.iter().map(x -> x % 3)))
  print(dict(xs.iter().map(x -> (x, x * x)).take(2)))
}
"),
    ("iterator_loops", "type Count = { at: Int, end: Int }

impl Iterator[Int] for Count {
  fn next(mut self) -> Option[Int] {
    if self.at >= self.end {
      return None
    }
    self.at += 1
    Some(self.at)
  }
}

impl FiniteIterator[Int] for Count {
  fn len(self) -> Natural {
    if self.end > self.at {
      self.end - self.at
    } else {
      0
    }
  }
}

fn main() {
  c := Count { at: 0, end: 6 }
  for x in c {
    if x == 2 {
      continue
    }
    if x == 5 {
      break
    }
    for y in c.take(2) {
      print(f\"{x} {y}\")
    }
  }
  print(c.at)
  print(c.len())
  print(c.to_list())
  print(c.fold(0, (a, b) -> a + b))
  print(c[2])
  words := [\"x\", \"yy\", \"zzz\"]
  for (i, w) in enumerate(words) {
    print(f\"{i}:{w}\")
  }
}
"),
    ("sized", "type Pixel = { r: U8, g: U8 }

fn rotr(x: U32, n: U32) -> U32 {
  x >> n | x << 32 - n
}

fn name(x: U8) -> String {
  match x {
    0 => \"zero\"
    255 => \"max\"
    _ => \"other\"
  }
}

fn main() {
  let a: U32 = 4294967295
  b := a + 1
  print(b)
  print(rotr(a, 7).to_int() + 5)
  print(rotr(2864434397, 13))
  let x: U8 = 200
  print(x + x)
  print(wrap_u8(300))
  print((-1).wrap_u32())
  let y: I8 = -128
  print(-y)
  print(y - 1)
  print(x / 7 + x % 7)
  let big: U64 = 18446744073709551615
  print(big)
  print(big + 1)
  print(big > 1)
  print(big.to_int() + 1)
  let m: I64 = -9223372036854775807
  print(m - 2)
  let n: I32 = -7
  print(n / 2)
  print(n % 2)
  print(n >> 1)
  print(n >> 40)
  let u: U32 = 1
  print(u << 31)
  print(u << 32)
  print(f\"x={u + 1} {name(0)} {name(255)} {name(7)}\")
  p := Pixel { r: 250, g: 10 }
  print(p)
  print(p.r + p.g)
  print(p == Pixel { r: 250, g: 10 })
  mut h: U32 = 0
  mut i := 0
  while i < 100 {
    h = h * 31 + 7
    i += 1
  }
  print(h)
}
"),
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
    ),    (
        "counting",
        "type Node = { name: String, size: Int, kids: Tree }

type Tree = Empty | Branch(Node, Tree)

fn label(n: Int) -> String {
    if n % 3 == 0 {
        return f\"fizz{n}\"
    }
    \"n\" + f\"{n}\"
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
            Empty => return \"none\",
            Branch(n, rest) => {
                if n.size > (1 << 70) + 6 {
                    return n.name
                }
                cur = rest
            },
        }
    }
    \"unreachable\"
}

fn names(t: Tree) -> String {
    match t {
        Empty => \"\",
        Branch(Node { name, .. }, Empty) => name,
        Branch(n, rest) => n.name + \",\" + names(rest),
    }
}

fn pick(x: (String, Int)) -> String {
    match x {
        (s, 0) | (s, 1) => s + \"!\",
        (_, n) => f\"{n}\",
    }
}

fn main() {
    t := build(20)
    print(names(t))
    print(first_big(t))
    mut s := \"a\"
    mut k := 0
    while k < 5 {
        s = s + label(k)
        k += 1
    }
    print(s)
    print(pick((\"x\" + \"y\", 1)))
    print(pick((\"x\", 7)))
    big := { (1 << 80) * 3 }
    print(big)
    shadow := \"one\"
    shadow := shadow + \"two\"
    print(shadow)
    p := { n := Node { name: \"inner\" + \"!\", size: 1, kids: build(2) }
        n.kids }
    print(p)
    print(if names(p) == \"n2,n1\" { \"yes\" + \"!\" } else { \"no\" })
}
",
    ),    (
        "held_operand",
        "type Box = { label: String }

fn both(a: Box, n: Int) -> String {
    f\"{a.label} {n}\"
}

fn main() {
    mut b := Box { label: \"first\" + \"!\" }
    print(both(b, {
        b = Box { label: \"second\" }
        1
    }))
    print(b)
}
",
    ),    (
        "lists",
        "type Item = { name: String, qty: Int }

fn total(items: List[Item]) -> Int {
    mut sum := 0
    for it in items {
        sum += it.qty
    }
    sum
}

fn squares(n: Int) -> List[Int] {
    mut out := []
    for i in 0..n {
        out = push(out, i * i)
    }
    out
}

fn describe(xs: List[Int]) -> String {
    match xs {
        [] => \"empty\",
        [x] => f\"one: {x}\",
        [a, b, ..rest] => f\"{a}, {b} and {len(rest)} more\",
    }
}

fn main() {
    xs := [3, 1, 4, 1, 5, 9, 2, 6]
    print(xs)
    print(len(xs))
    print(xs[2])
    print(xs[2..5])
    print(xs[..3])
    print(xs[6..])
    print(xs[1..=2])
    print(xs.len())
    print(get(xs, 1))
    print(get(xs, 10))
    print(take(xs, 2))
    print(drop(xs, 6))
    print(xs + [7, 8])
    print(squares(6))
    print(describe([]))
    print(describe([42]))
    print(describe(xs))
    items := [Item { name: \"tea\", qty: 2 }, Item { name: \"cake\", qty: 3 }]
    print(items)
    print(total(items))
    names := [\"a\" + \"b\", \"c\"]
    print(names)
    print(names == [\"ab\", \"c\"])
    print([[1, 2], [3]] == [[1, 2], [3]])
    print([1, 2] == [1, 3])
    print([(1, \"x\"), (2, \"y\")])
    mut acc := \"\"
    for (n, s) in [(1, \"x\"), (2, \"y\")] {
        if n == 2 {
            break
        }
        acc = acc + s
    }
    print(acc)
    mut count := 0
    for i in 1..=10 {
        if i % 2 == 0 {
            continue
        }
        count += i
    }
    print(count)
    big := [1 << 70, 2]
    print(big)
    print(range(2, 5))
    let ys: List[String] = []
    print(push(ys, \"z\"))
}
",
    ),
    (
        "list_index_out_of_range",
        "fn main() {
    xs := [1, 2, 3]
    print(xs[1])
    print(xs[3])
}
",
    ),
    (
        "closures_and_generics",
        "type Pair[A, B] = { first: A, second: B }

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
    print(twice(\"ab\", s -> s + s))
    print(pair_of(1, \"one\"))
    print(pair_of([1.5], Some(true)))
    print(first_or([7, 8], 0))
    print(first_or([], \"none\"))
    add5 := adder(5)
    print(add5(1))
    print(twice(1, adder(100)))
    xs := [1, 2, 3, 4, 5, 6]
    suffix := \"!\"
    print(map(xs, x -> x * x))
    print(xs.map(x -> f\"{x}{suffix}\"))
    print(filter(xs, is_even))
    print(xs.filter(x -> x > 3).map(x -> x - 3))
    print(fold(xs, 0, (acc, x) -> acc + x))
    print(all(xs, x -> x > 0))
    print(any(xs, x -> x > 5))
    print(find(xs, x -> x > 4))
    print(count(xs, is_even))
    print(contains(xs, 4))
    print(reverse(xs))
    print(join([\"a\", \"b\", \"c\"], \"-\"))
    print(zip(xs, [\"a\", \"b\"]))
    print(enumerate([\"x\", \"y\"]))
    print(sum(xs))
    print(sum([1.5, 2.25]))
    print([x * 2 for x in xs where x % 3 == 0])
    print([(a, b) for a in [1, 2] for b in [\"p\", \"q\"]])
    print([n for n in 0..4])
    names := [Pair { first: \"z\", second: 1 }, Pair { first: \"y\", second: 2 }]
    print(names.map(p -> p.first))
    mut total := 0
    for f in [adder(1), adder(2), x -> x * 3] {
        total += f(10)
    }
    print(total)
}
",
    ),
    (
        "generic_result_wanted",
        "fn empty[T]() -> List[T] {
    []
}

fn nothing[T]() -> Option[T] {
    None
}

fn main() {
    let xs: List[String] = empty()
    print(push(xs, \"a\" + \"b\"))
    let o: Option[List[Int]] = nothing()
    print(o)
    mut ys: List[Int] = empty()
    ys = push(ys, 3)
    print(ys)
}
",
    ),
    (
        "builtins_sets_dicts_formats",
        "type Card = { suit: String, rank: Int }

type Level = Low | Mid(Int) | High { why: String }

fn main() {
    s := \"  Hello, World  \"
    t := trim(s)
    print(t)
    print(split(\"a,b,,c\", \",\"))
    print(contains(t, \"World\"))
    print(starts_with(t, \"Hell\"))
    print(ends_with(t, \"x\"))
    print(index_of(t, \"o\"))
    print(index_of(t, \"z\"))
    print(replace(t, \"l\", \"L\"))
    print(repeat(\"ab\", 3))
    print(reverse(\"héllo\"))
    print(chars(\"añb\"))
    print(lines(\"one\\ntwo\\r\\nthree\\n\"))
    print(ord(\"A\"))
    print(chr(97))
    print(byte_len(\"é\"))
    print(byte_at(\"A\", 0))
    print(byte_slice(\"hello\", 1, 3))
    print(parse_int(\"42\"))
    print(parse_int(\"123456789012345678901234567890\"))
    print(parse_int(\"x\"))
    print(parse_float(\"2.5\"))
    print(to_string(12) + debug(\"q\"))
    print(abs(-5))
    print(abs(-2.5))
    print(min(3, 7))
    print(max(2.5, 1.5))
    print(float(7))
    print(floor(2.7))
    print(ceil(2.1))
    print(round(2.5))
    print(sqrt(16.0))
    print(checked_div(1.0, 0.0))
    print(checked_add(1.0, 2.0))
    print(checked_sqrt(-1.0))
    print(checked_floor(3.9))
    print(float_bits(1.0))
    print(float_from_bits(4607182418800017408))
    print(sort([5, 3, 9, 1, 3]))
    print(sort([\"pear\", \"apple\", \"fig\"]))
    print(sort([2.5, -1.0, 0.0]))
    print(sort([(2, \"b\"), (1, \"z\"), (2, \"a\")]))
    print(sort([High { why: \"x\" }, Mid(3), Low, Mid(1)]))
    print(sort_by([Card { suit: \"h\", rank: 3 }, Card { suit: \"s\", rank: 1 }], c -> c.rank))
    ns := Set { 3, 1, 2, 3 }
    print(ns)
    print(len(ns))
    print(contains(ns, 2))
    print(insert(ns, 0))
    print(remove(ns, 3))
    print(union(ns, Set { 9, 1 }))
    print(intersection(ns, Set { 2, 3, 4 }))
    print(difference(ns, Set { 1 }))
    print(to_list(ns))
    print(set([\"b\", \"a\", \"b\"]))
    ages := Dict { \"bo\": 30, \"al\": 25 }
    print(ages)
    print(get(ages, \"al\"))
    print(get(ages, \"zz\"))
    print(contains(ages, \"bo\"))
    print(insert(ages, \"cy\", 41))
    print(insert(ages, \"al\", 26))
    print(remove(ages, \"bo\"))
    print(keys(ages))
    print(values(ages))
    print(items(ages))
    print(map_values(ages, a -> a + 1))
    mut total := 0
    for (name, age) in ages {
        total += age
    }
    print(total)
    for n in ns {
        print(n)
    }
    let empty: Dict[String, Int] = Dict {}
    print(empty)
    print(Set[Int] {})
    print(ns == Set { 1, 2, 3 })
    print(Set { x for x in [1, 2, 2, 3] })
    pi := 3.14159
    n := 42
    w := \"hi\"
    print(f\"[{pi:.2}] [{n:>6}] [{n:<6}] [{n:^6}] [{n:06}] [{n:+}] [{w:*>5}] [{w:.1}]\")
}
",
    ),
    (
        "unicode",
        "fn main() {
    for s in [\"hello World\", \"straße\", \"ΟΔΥΣΣΕΥΣ\", \"İstanbul\", \"ǅ\", \"ﬃ\", \"123\", \"١٢٣\", \"  \\t\", \"\", \"abc1\", \"日本語\", \"Ünïcödé\"] {
        print(f\"{s}: {to_upper(s)} {to_lower(s)} {is_digit(s)} {is_alpha(s)} {is_space(s)}\")
    }
}
",
    ),
    (
        "methods_and_traits",
        "type P = { x: Int, y: Int }

type Circle = { r: Float }

type Square = { side: Float }

type Box[T] = { item: T }

trait Shape {
    fn area(self) -> Float

    fn name(self) -> String

    fn describe(self) -> String {
        f\"{self.name()} of area {self.area()}\"
    }
}

trait Named {
    fn label(self) -> String
}

impl P {
    fn origin() -> P {
        P { x: 0, y: 0 }
    }

    fn plus(self, o: P) -> P {
        P { x: self.x + o.x, y: self.y + o.y }
    }

    fn norm1(self) -> Int {
        abs(self.x) + abs(self.y)
    }
}

impl Display for P {
    fn to_string(self) -> String {
        f\"<{self.x}, {self.y}>\"
    }
}

impl Shape for Circle {
    fn area(self) -> Float {
        3.0 * self.r * self.r
    }

    fn name(self) -> String {
        \"circle\"
    }
}

impl Shape for Square {
    fn area(self) -> Float {
        self.side * self.side
    }

    fn name(self) -> String {
        \"square\"
    }

    fn describe(self) -> String {
        f\"a square, side {self.side}\"
    }
}

impl Named for Circle {
    fn label(self) -> String {
        \"C\"
    }
}

impl[T] Box[T] {
    fn get(self) -> T {
        self.item
    }

    fn map[U](self, f: (T) -> U) -> Box[U] {
        Box { item: f(self.item) }
    }
}

fn total_area[T: Shape](xs: List[T]) -> Float {
    fold(xs, 0.0, (acc, s) -> acc + s.area())
}

fn biggest(shapes: List[Shape]) -> String {
    mut best := \"\"
    mut most := -1.0
    for s in shapes {
        if s.area() > most {
            most = s.area()
            best = s.describe()
        }
    }
    best
}

fn main() {
    p := P.origin().plus(P { x: 3, y: -4 })
    print(p)
    print(f\"p = {p}, norm {p.norm1()}\")
    print([p, P { x: 1, y: 1 }])
    print(Circle { r: 1.0 }.describe())
    print(Square { side: 2.0 }.describe())
    print(Circle { r: 2.0 }.label())
    b := Box { item: 21 }
    print(b.get())
    print(b.map(x -> x * 2).get())
    print(b.map(x -> f\"#{x}\"))
    print(total_area([Circle { r: 1.0 }, Circle { r: 2.0 }]))
    let shapes: List[Shape] = [Circle { r: 1.0 }, Square { side: 3.0 }, Circle { r: 0.5 }]
    print(biggest(shapes))
    for s in shapes {
        print(s.name())
    }
    print(shapes)
    let one: Shape = Square { side: 1.0 }
    print(one == Square { side: 1.0 })
    print(one.describe())
}
",
    ),
    (
        "failures_as_values",
        "type Problem = | Empty | TooBig { max: Int }

fn parse(s: String) -> Result[Int, String] {
    match parse_int(s) {
        Some(n) => Ok(n),
        None => Err(f\"not a number: {s}\"),
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
    print(sum_two(\"2\", \"40\"))
    print(sum_two(\"2\", \"x\"))
    print(first_even([1, 3, 4]))
    print(first_even([1, 3]))
    print(parse(\"7\") except { 0 })
    print(parse(\"q\") except e { byte_len(e) })
    print(check(500) except e {
        match e {
            Empty => -1,
            TooBig { max } => max,
        }
    })
    print(get([1, 2], 5) ?? 99)
    print(get([1, 2], 1) ?? 99)
    print(parse_int(\"12\") ?? 0)
    n := parse(\"8\")!
    print(n)
    bad := parse(\"nope\")!
    print(bad)
    Ok(())
}
",
    ),
    (
        "main_result_code",
        "fn main() -> Result[Int, String] {
    print(\"exiting with 3\")
    Ok(3)
}
",
    ),
    (
        "mutation",
        "type Inner = { y: Int, tag: String }

type Outer = { x: Int, inner: Inner, items: List[String] }

type Counter = { n: Int, log: List[String] }

impl Counter {
    fn bump(mut self, by: Int) {
        self.n += by
        self.log = push(self.log, f\"+{by}\")
    }

    fn total(self) -> Int {
        self.n
    }
}

fn push_twice(mut xs: List[Int], x: Int) {
    xs = push(xs, x)
    xs = push(xs, x * 10)
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
    mut o := Outer { x: 1, inner: Inner { y: 2, tag: \"a\" }, items: [] }
    keep := o
    o.x = 10
    o.inner.y = 20
    o.items = push(o.items, \"first\" + \"!\")
    print(o)
    print(keep)
    rename(mut o, \"renamed\")
    print(o.inner)
    mut c := Counter { n: 0, log: [] }
    mut c.bump(3)
    mut c.bump(4)
    print(c.total())
    print(c)
    mut p := \"left\"
    mut q := \"right\"
    swap(mut p, mut q)
    print(f\"{p} {q}\")
    mut pair := (1, \"one\")
    pair.0 = 2
    print(pair)
}
",
    ),
    (
        "unions_and_distinct",
        "type Circle = { r: Float }

type Shape = Circle | Square { side: Float } | Dot

type Id = Int | String

distinct type UserId = Int

distinct type Label = String

fn area(s: Shape) -> Float {
    match s {
        Circle { r } => 3.0 * r * r,
        Square { side } => side * side,
        Dot => 0.0,
    }
}

fn describe(id: Id) -> String {
    match id {
        Int => f\"number {id + 1}\",
        String => f\"text {id}\",
    }
}

fn small(n: Natural) -> Natural {
    n * 2
}

fn main() {
    let shapes: List[Shape] = [Circle { r: 1.0 }, Square { side: 2.0 }, Dot]
    for s in shapes {
        print(area(s))
    }
    print(shapes)
    let a: Id = 41
    let b: Id = \"hi\"
    print(describe(a))
    print(describe(b))
    print(a)
    print(b)
    print([a, b])
    print(a == b)
    let c: Id = 41
    print(a == c)
    print(sort([b, a, c]))
    if b is String {
        print(f\"{b}!\")
    }
    print(a is Int)
    u := UserId(7)
    print(u)
    print(u.value + 1)
    print(u == UserId(7))
    print(sort([UserId(9), UserId(3)]))
    l := Label(\"x\")
    print(f\"{l} {l.value}\")
    print(small(4))
    let one: Shape = Dot
    print(one == Dot)
}
",
    ),
    (
        "contract_1",
        "fn half(n: Int) -> Int
    requires n % 2 == 0
{
    n / 2
}

fn main() {
    print(half(4))
    print(half(read_odd()))
}

fn read_odd() -> Int {
    parse_int(\"3\") ?? 0
}
",
    ),
    (
        "contract_2",
        "fn pos(n: Int) -> Int
    ensures it > 0
{
    n - 10
}

fn main() {
    print(pos(read(20)))
    print(pos(read(5)))
}

fn read(n: Int) -> Int {
    parse_int(f\"{n}\") ?? 0
}
",
    ),
    (
        "contract_3",
        "type Small = Int where it < 10

fn f(x: Small) -> Small {
    x + 1
}

fn main() {
    print(f(read(3)))
    print(f(read(9)))
}

fn read(n: Int) -> Int {
    parse_int(f\"{n}\") ?? 0
}
",
    ),
    (
        "contract_4",
        "fn g(n: Natural) -> Int {
    n
}

fn main() {
    print(g(read(3)))
    let k: Positive = read(0)
    print(k)
}

fn read(n: Int) -> Int {
    parse_int(f\"{n}\") ?? 0
}
",
    ),
    (
        "contract_5",
        "type Range = { lo: Int, hi: Int } where it.lo <= it.hi

fn main() {
    r := Range { lo: 1, hi: read(5) }
    print(r)
    q := Range { lo: 1, hi: read(0) }
    print(q)
}

fn read(n: Int) -> Int {
    parse_int(f\"{n}\") ?? 0
}
",
    ),
    (
        "contract_11",
        "fn g(n: Natural) -> Int {
    n
}

fn main() {
    print(g(read(-1)))
}

fn read(n: Int) -> Int {
    parse_int(f\"{n}\") ?? 0
}
",
    ),
    (
        "contract_12",
        "type Item = { name: String, qty: Positive }

fn main() {
    print(Item { name: \"a\", qty: read(0) })
}

fn read(n: Int) -> Int {
    parse_int(f\"{n}\") ?? 0
}
",
    ),
    (
        "contract_13",
        "fn main() {
    mut k: Natural = 3
    k = k - read(5)
    print(k)
}

fn read(n: Int) -> Int {
    parse_int(f\"{n}\") ?? 0
}
",
    ),
    (
        "contract_14",
        "fn main() {
    mut i := 0
    while i < 5
        invariant i <= read(3)
    {
        i += 1
    }
}

fn read(n: Int) -> Int {
    parse_int(f\"{n}\") ?? 0
}
",
    ),
    (
        "contract_15",
        "fn total(xs: List[Natural]) -> Int {
    fold(xs, 0, (a, x) -> a + x)
}

fn main() {
    print(total([1, read(-2)]))
}

fn read(n: Int) -> Int {
    parse_int(f\"{n}\") ?? 0
}
",
    ),
    (
        "contract_16",
        "fn h(x: Int where it > 5) -> Int {
    x
}

fn main() {
    print(h(read(2)))
}

fn read(n: Int) -> Int {
    parse_int(f\"{n}\") ?? 0
}
",
    ),
    (
        "contract_17",
        "distinct type Age = Int where it >= 0

fn main() {
    print(Age(read(4)))
    print(Age(read(-4)))
}

fn read(n: Int) -> Int {
    parse_int(f\"{n}\") ?? 0
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
    // (a refined option read by a derived `CommandLine`: std.cli checks it with compile-time code)
    let file = dir.join("options.strata");
    save(&file, "from std.cli use CommandLine\n\ntype Options = { times: Int where 1 <= it = 1 }\n\nderive CommandLine for Options\n\nfn main(opts: Options) {\n    print(opts.times)\n}\n");
    let out = compiler::run(&["codegen".to_string(), file.to_string_lossy().into_owned()]);
    assert!(out.starts_with("error: in `"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `codegen --proofs FILE` (what `strata build --strata` hands it from
/// `verify`, as `llvm::proofs_text` writes it) leaves out the checks proven:
/// an argument's and a precondition every call proves, a result's, a
/// `let`'s, an index's, and a division's check for 0.
#[test]
fn leaves_out_what_verify_proved() {
    let dir = scratch("proven");
    let file = dir.join("proven.strata");
    save(
        &file,
        "fn half(n: Natural) -> Natural {\n    n * 2\n}\n\nfn pick(xs: List[Int], i: Int) -> Int\n    requires 0 <= i && i < len(xs)\n{\n    xs[i]\n}\n\nfn safe(a: Int, b: Int) -> Int {\n    if b == 0 {\n        0\n    } else {\n        a / b\n    }\n}\n\nfn main() {\n    let x: Positive = 3\n    print(half(x) + pick([1, 2, 3], 1) + safe(7, 2))\n}\n",
    );
    let proofs = dir.join("proofs.txt");
    std::fs::write(&proofs, "proven\thalf\tresult of `half`: Natural\nproven\tsafe\tno division by zero\nproven\tmain\t`let` annotated `Positive`\nproven\tpick\tindex in range\nentry\thalf\targument n\nentry\tpick\trequires 0\n").unwrap();
    let checks = |ir: &str| ["does not satisfy", "precondition of", "out of range", "@strata_div("].iter().filter(|m| ir.contains(*m)).count();
    let all = compiler::run(&["codegen".to_string(), file.to_string_lossy().into_owned()]);
    assert_eq!(checks(&all), 4, "{all}");
    let proven = compiler::run(&["codegen".to_string(), "--proofs".to_string(), proofs.to_string_lossy().into_owned(), file.to_string_lossy().into_owned()]);
    assert_eq!(checks(&proven), 0, "{proven}");
    assert!(proven.contains("@strata_div_nz("), "{proven}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A package calling C through `extern c` (its `[native]` C file and libm):
/// sized integers, floats, booleans and strings across, handles (an
/// `Option` of one for NULL), `mut` out-pointers, contracts.
#[test]
fn foreign_calls_do_what_rust_compiled_ones_do() {
    if !have_clang() {
        return;
    }
    let dir = scratch("ffi");
    let pkg = dir.join("ffi");
    std::fs::create_dir_all(pkg.join("c")).unwrap();
    std::fs::write(pkg.join("strata.toml"), "name = \"ffi\"\n\n[native]\nsources = [\"c/lib.c\"]\nlibs = [\"m\"]\n").unwrap();
    std::fs::write(pkg.join("c/lib.c"), "#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <stdbool.h>

int32_t add32(int32_t a, int32_t b) { return a + b; }
double half(double x) { return x / 2; }
bool is_even(int64_t n) { return n % 2 == 0; }
uint8_t low_byte(uint32_t x) { return (uint8_t)(x & 0xff); }
int8_t negate8(int8_t x) { return (int8_t)-x; }
const char *greet(const char *name) {
  static char buf[256];
  snprintf(buf, sizeof buf, \"hello, %s!\", name);
  return buf;
}
size_t name_len(const char *s) { return strlen(s); }
typedef struct { int32_t n; } Counter;
Counter *counter_new(int32_t start) { Counter *c = malloc(sizeof *c); c->n = start; return c; }
void counter_bump(Counter *c) { c->n++; }
int32_t counter_get(Counter *c) { return c->n; }
void counter_free(Counter *c) { free(c); }
Counter *maybe_counter(bool make) { return make ? counter_new(7) : NULL; }
void double_it(int32_t *x) { *x *= 2; }
void reset_counter(Counter **c) { counter_free(*c); *c = counter_new(100); }
").unwrap();
    save(&pkg.join("main.strata"), "extern c type Counter

extern c fn add32(a: I32, b: I32) -> I32

extern c fn half(x: Float) -> Float

extern c fn is_even(n: I64) -> Bool

extern c fn low_byte(x: U32) -> U8

extern c fn negate8(x: I8) -> I8

extern c fn greet(name: String) -> String

extern c fn name_len(s: String) -> U64

extern c fn counter_new(start: I32) -> Counter

extern c fn counter_bump(c: Counter)

extern c fn counter_get(c: Counter) -> I32

extern c fn counter_free(c: Counter)

extern c fn maybe_counter(make: Bool) -> Option[Counter]

extern c fn double_it(mut x: I32)

extern c fn reset_counter(mut c: Counter)

extern c fn cbrt(x: Float) -> Float

extern c fn positive(x: I32) -> I32
  requires x > 0
  ensures it > 0

fn main() {
  print(add32(40, 2))
  print(half(5.0))
  print(is_even(10))
  print(is_even(7))
  print(low_byte(4660))
  print(negate8(-128))
  print(greet(\"Ada\"))
  print(name_len(\"four\"))
  c := counter_new(1)
  counter_bump(c)
  counter_bump(c)
  print(counter_get(c))
  print(c == c)
  mut d := counter_new(5)
  reset_counter(mut d)
  print(counter_get(d))
  counter_free(d)
  counter_free(c)
  match maybe_counter(true) {
    Some(m) => {
      print(counter_get(m))
      counter_free(m)
    },
    None => print(\"none\"),
  }
  print(maybe_counter(false) is None)
  mut x: I32 = 21
  double_it(mut x)
  print(x)
  print(cbrt(27.0))
}
");
    let rust = dir.join("ffi-rust");
    let o = Command::new(STRATA).arg("build").arg(&pkg).arg("-o").arg(&rust).output().unwrap();
    assert!(o.status.success(), "strata build: {}", String::from_utf8_lossy(&o.stderr));
    let ir = compiler::run(&["codegen".to_string(), pkg.to_string_lossy().into_owned()]);
    assert!(!ir.starts_with("error: "), "not compiled by compiler/codegen:\n{ir}");
    let ll = dir.join("ffi.ll");
    std::fs::write(&ll, &ir).unwrap();
    let ours = dir.join("ffi-strata");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/llvm/runtime.c");
    let o = Command::new(cc()).args(["-O2", "-Wno-override-module", "-o"]).arg(&ours).arg(&ll).arg(&runtime).arg(pkg.join("c/lib.c")).arg("-lm").output().unwrap();
    assert!(o.status.success(), "clang refused compiler/codegen's IR: {}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(outcome(&rust), outcome(&ours));
    let m = memory(&ours);
    assert!(m.ends_with(" 0 live"), "compiler/codegen's build leaks: {m}");
    let _ = std::fs::remove_dir_all(&dir);
}
