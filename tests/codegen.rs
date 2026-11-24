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

/// The program at `file` built by compiler/codegen (its IR linked with the C
/// runtime by clang): the executable.
fn ours(dir: &Path, name: &str, file: &Path) -> PathBuf {
    let ir = compiler::run(&["codegen".to_string(), file.to_string_lossy().into_owned()]);
    assert!(!ir.starts_with("error: "), "{name}: not compiled by compiler/codegen:\n{ir}");
    let ll = dir.join(format!("{name}.ll"));
    std::fs::write(&ll, &ir).unwrap();
    let ours = dir.join(format!("{name}-strata"));
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/llvm/runtime.c");
    let o = Command::new(cc()).args(["-O2", "-Wno-override-module", "-o"]).arg(&ours).arg(&ll).arg(&runtime).arg("-lm").output().unwrap();
    assert!(o.status.success(), "{name}: clang refused compiler/codegen's IR: {}", String::from_utf8_lossy(&o.stderr));
    ours
}

/// The program at `file` built by Rust and by Strata: their outcomes.
fn both(dir: &Path, name: &str, file: &Path) -> ((String, String, Option<i32>), (String, String, Option<i32>)) {
    let rust = dir.join(format!("{name}-rust"));
    let o = Command::new(STRATA).arg("build").arg(file).arg("-o").arg(&rust).output().unwrap();
    assert!(o.status.success(), "{name}: strata build: {}", String::from_utf8_lossy(&o.stderr));
    let ours = ours(dir, name, file);
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
    ("refined_options", "from std.cli use CommandLine
from std.io use Console

/// Greet people by name.
type Options = {
  /// Shout the greeting.
  loud: Bool = false,
  /// Say it this many times.
  times: Int where 1 <= it && it <= 10 = 1,
  /// Who to greet (the world if nobody).
  names: List[String] = [],
}

derive CommandLine for Options

fn main(opts: Options, mut console: Console) {
  who := if len(opts.names) == 0 {
    \"world\"
  } else {
    join(opts.names, \" and \")
  }
  for _ in 0..opts.times {
    text := f\"hello, {who}!\"
    mut console.print(if opts.loud {
      to_upper(text)
    } else {
      text
    })
  }
  mut going := true
  while going {
    match mut console.read_line() {
      Some(l) => print(\"read \" + l),
      None => {
        going = false
      },
    }
  }
}
"),
    ("regex_matches", "fn main() {
  print(matches(\"abc123\", \"[a-z]+[0-9]+\"))
  print(matches(\"abc\", \"[0-9]+\"))
  p := \"(\" + \"x\"
  print(matches(\"x\", p))
}
"),
    ("container_refinements", "fn first_pos(xs: List[Int]) -> Option[Positive] {
  for x in xs {
    if x > 0 {
      return Some(x)
    }
  }
  None
}

fn main() {
  print(first_pos([-1, 3]))
  let s: Set[Natural] = Set { 1, 2 }
  print(s)
  let d: Dict[String, Positive] = Dict { \"a\": 1 }
  print(d)
  let r: Result[Natural, String] = Ok(4)
  print(r)
}
"),
    ("container_refinement_fault", "fn first_pos(xs: List[Int]) -> Option[Positive] {
  for x in xs {
    if x > 0 {
      return Some(x)
    }
  }
  None
}

fn main() {
  print(first_pos([-1, 3]))
  let s: Set[Natural] = Set { 1, 2 }
  print(s)
  let d: Dict[String, Positive] = Dict { \"a\": 1 }
  print(d)
  let r: Result[Natural, String] = Ok(4)
  print(r)
  let bad: Dict[String, Positive] = Dict { \"b\": 0 }
  print(bad)
}
"),
    ("refined_record_assigned", "type Range = { lo: Int, hi: Int } where it.lo <= it.hi

fn main() {
  mut r := Range { lo: 1, hi: 5 }
  r.lo = 3
  print(r)
  print(r)
}
"),
    ("refined_record_broken", "type Range = { lo: Int, hi: Int } where it.lo <= it.hi

fn main() {
  mut r := Range { lo: 1, hi: 5 }
  r.lo = 3
  print(r)
  r.lo = 9
  print(r)
}
"),
    ("refined_union", "type Small = Int where it < 10

type Id = Small | String

fn show_id(i: Id) -> String {
  match i {
    String => \"text \" + i,
    _ => \"small\",
  }
}

fn main() {
  print(show_id(3))
  print(show_id(\"x\"))
  let j: Id = 5
  print(j)
  mut k: Id = \"a\"
  k = 7
  print(k)
  k = \"b\" + \"c\"
  print(k)
}
"),
    ("refined_union_broken", "type Small = Int where it < 10

type Id = Small | String

fn show_id(i: Id) -> String {
  match i {
    String => \"text \" + i,
    _ => \"small\",
  }
}

fn main() {
  print(show_id(30))
  print(show_id(\"x\"))
  let j: Id = 50
  print(j)
}
"),
    ("iterator_parameter", "fn total(it: Iterator[Int]) -> Int {
  mut t := 0
  for x in it {
    t += x
  }
  t
}

fn main() {
  print(total([1, 2].iter()))
  print(total([1, 2, 3].iter().map(x -> x * 10)))
}
"),
    ("supervised_string_panic", "fn risky(s: String, n: Int) -> String {
  repeat(s, n)
}

fn main() {
  r := scope {
    a := supervised spawn risky(\"ab\", 0 - 1)
    b := supervised spawn risky(\"ab\", 2)
    (await a, await b)
  }
  match r.0 {
    Ok(x) => print(x),
    Err(p) => print(p.kind),
  }
  print(r.1)
}
"),
    ("iterator_let", "fn main() {
  let it: Iterator[Int] = [1, 2].iter()
  for x in it {
    print(x)
  }
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

/// A union with refined members checks a `let`'s value too (`llvm.rs`
/// checks it only where the value is already a union: an argument, a
/// result).
#[test]
fn a_let_of_a_union_with_refined_members_is_checked() {
    if !have_clang() {
        return;
    }
    let dir = scratch("refined-let");
    let file = dir.join("refined_let.strata");
    save(&file, "type Small = Int where it < 10\n\ntype Id = Small | String\n\nfn main() {\n    let j: Id = 5\n    print(j)\n    let k: Id = 50\n    print(k)\n}\n");
    let (out, err, code) = outcome(&ours(&dir, "refined_let", &file));
    assert_eq!(out, "5\n");
    assert!(err.starts_with("fault (Refinement): let binding: 50 is not a `Small | String`"), "{err}");
    assert_eq!(code, Some(101));
    let _ = std::fs::remove_dir_all(&dir);
}

/// `strata build` and `strata test` are compiler/codegen's by default (the
/// tests run with `STRATA_BACKEND=rust`, from .cargo/config.toml, so it is
/// removed here): a program it compiles is built by it; one it doesn't
/// cover yet is built by Rust, with a note.
#[test]
fn build_and_test_use_compiler_codegen_by_default() {
    if !have_clang() {
        return;
    }
    let dir = scratch("default");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let default = |args: &[&str]| {
        let o = Command::new(STRATA).current_dir(&dir).args(args).env_remove("STRATA_BACKEND").env("STRATA_COMPILER", root.join("compiler")).output().unwrap();
        (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned(), o.status.success())
    };
    let file = dir.join("ok.strata");
    save(&file, "fn double(n: Int) -> Int {\n    n * 2\n}\n\ntest \"double\" {\n    assert double(2) == 4\n}\n\nfn main() {\n    print(double(21))\n}\n");
    let (out, err, ok) = default(&["build", "ok.strata", "-o", "ok"]);
    assert!(ok && out.contains("(compiler/codegen)"), "{out}{err}");
    assert_eq!(outcome(&dir.join("ok")).0, "42\n");
    let (out, err, ok) = default(&["--file", "ok.strata", "test"]);
    assert!(ok && out.contains("✓ double") && out.contains("1 passed, 0 failed"), "{out}{err}");
    // (not covered yet: built by Rust)
    let file = dir.join("field.strata");
    save(&file, "type Holder = { it: Iterator[Int] }\n\nfn main() {\n    h := Holder { it: [1, 2].iter() }\n    for x in h.it {\n        print(x)\n    }\n}\n");
    let (out, err, ok) = default(&["build", "field.strata", "-o", "field"]);
    assert!(ok && !out.contains("(compiler/codegen)") && err.contains("using llvm.rs"), "{out}{err}");
    assert_eq!(outcome(&dir.join("field")).0, "1\n2\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn refuses_what_it_does_not_compile_yet() {
    let dir = scratch("refused");
    // (an iterator in a record's field of a trait type: its element type is lost)
    let file = dir.join("iterator_field.strata");
    save(&file, "type Holder = { it: Iterator[Int] }\n\nfn main() {\n    h := Holder { it: [1, 2].iter() }\n    for x in h.it {\n        print(x)\n    }\n}\n");
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
    // (a header from `include_dirs`)
    std::fs::create_dir_all(pkg.join("include")).unwrap();
    std::fs::write(pkg.join("include/greeting.h"), "#define GREETING \"hello, %s!\"\n").unwrap();
    std::fs::write(pkg.join("strata.toml"), "name = \"ffi\"\n\n[native]\nsources = [\"c/lib.c\"]\nlibs = [\"m\"]\ninclude_dirs = [\"include\"]\n").unwrap();
    std::fs::write(pkg.join("c/lib.c"), "#include <greeting.h>
#include <stdint.h>
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
  snprintf(buf, sizeof buf, GREETING, name);
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
    let o = Command::new(cc()).args(["-O2", "-Wno-override-module", "-o"]).arg(&ours).arg(&ll).arg(&runtime).arg(format!("-I{}", pkg.join("include").display())).arg(pkg.join("c/lib.c")).arg("-lm").output().unwrap();
    assert!(o.status.success(), "clang refused compiler/codegen's IR: {}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(outcome(&rust), outcome(&ours));
    let m = memory(&ours);
    assert!(m.ends_with(" 0 live"), "compiler/codegen's build leaks: {m}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `strata test --compare`: the tests compiled by compiler/codegen
/// (`codegen --tests`) and run as `strata test` runs its own give the same
/// report (passing, failing with what they printed, the console, a virtual
/// clock, a fault).
#[test]
fn tests_run_as_strata_test_runs_them() {
    let dir = scratch("tests");
    let file = dir.join("tests.strata");
    save(&file, "from std.io use Console, recorder, printed
from std.time use VirtualClock, Clock, seconds

fn double(n: Int) -> Int {
  n * 2
}

fn greet(mut c: Console, who: String) {
  mut c.print(\"hi \" + who)
}

test \"doubles\" {
  assert double(2) == 4
}

test \"fails on purpose\" {
  print(\"about to fail\")
  assert double(3) == 7
}

test \"records\" {
  mut r := recorder([])
  greet(mut r, \"Ada\")
  assert printed(r) == [\"hi Ada\"]
}

test \"uses the console\" (mut console: Console) {
  greet(mut console, \"you\")
}

test \"sleeps virtually\" (clock: VirtualClock) {
  clock.sleep(seconds(5))
  assert clock.now().value == seconds(5).value
}

test \"index out of range\" {
  xs := [1, 2]
  print(xs[double(2)])
}

fn main() {
  print(double(1))
}
");
    let o = Command::new(STRATA).arg("--file").arg(&file).args(["test", "--compare"]).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("compare: they agree"), "{out}{}", String::from_utf8_lossy(&o.stderr));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Mocks (`mock T`): their methods' results generated (refined types,
/// records, lists, options, floats) until their `ensures` hold, their
/// `requires` checked; the same report as `strata test`'s.
#[test]
fn mocks_as_strata_test_runs_them() {
    let dir = scratch("mocks");
    let file = dir.join("mocks.strata");
    save(&file, "type Score = Int where 0 <= it && it <= 100

type Item = { name: String, qty: Int where it > 0 }

trait Store {
  fn price(self, sku: String) -> Int
    ensures it >= 0
  fn score(self) -> Score
  fn items(self, n: Int) -> List[Item]
    requires n >= 0
  fn pick(self) -> Option[Bool]
  fn ratio(self) -> Float
}

fn total(s: Store, skus: List[String]) -> Int {
  skus.map(k -> s.price(k)).sum()
}

test \"prices are never negative\" {
  s := mock Store
  assert total(s, [\"a\", \"b\", \"c\"]) >= 0
}

test \"scores are in range\" {
  s := mock Store
  for _ in 0..20 {
    x := s.score()
    assert 0 <= x && x <= 100
  }
}

test \"items have quantities\" {
  s := mock Store
  for it in s.items(3) {
    assert it.qty > 0
  }
}

test \"shows as a mock\" {
  s := mock Store
  o := s.pick()
  assert o == None || o == Some(true) || o == Some(false)
  r := s.ratio()
  assert r == r
}

test \"requires is checked\" {
  s := mock Store
  print(len(s.items(-1)))
}

fn main() {
  print(1)
}
");
    let o = Command::new(STRATA).arg("--file").arg(&file).args(["test", "--compare"]).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("compare: they agree"), "{out}{}", String::from_utf8_lossy(&o.stderr));
    let _ = std::fs::remove_dir_all(&dir);
}

/// `verify --strata`: `compiler verify` (what the solver leaves fuzzed by
/// compiler/codegen's drivers in the compiler's JIT) gives the same
/// verdicts, cases and counterexamples as `verify --no-race`
/// (postconditions seeing the parameters); `--compare` says they agree.
#[test]
fn fuzzing_as_verify_fuzzes() {
    let dir = scratch("fuzz");
    let file = dir.join("fuzz.strata");
    save(&file, "type Point = { x: Float, y: Float }

fn half(x: Float) -> Float
  ensures it <= x || x < 0.0
{
  x / 2.0
}

fn norm1(p: Point) -> Float
  ensures it >= 0.0
{
  abs(p.x) + abs(p.y)
}

fn wrong(x: Float) -> Float
  ensures it > x
{
  x * 2.0
}

fn main() {
  print(half(3.0))
}
");
    // (the compiler built once, in the cache: verdicts not kept, so both run every check)
    let run = |how: &str| {
        let o = Command::new(STRATA).arg("--file").arg(&file).args(["verify", how, "--no-cache"]).env("STRATA_COMPILER", Path::new(env!("CARGO_MANIFEST_DIR")).join("compiler")).output().unwrap();
        // (stdout: stderr may say the compiler is being built)
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let (rust, ours) = (run("--no-race"), run("--strata"));
    assert!(rust.contains("found by the fuzzer"), "{rust}");
    assert_eq!(rust, ours);
    let compared = run("--compare");
    assert!(compared.contains("compare: they agree"), "{compared}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `compiler codegen --core-rt`: core.rt (the runtime written in Strata:
/// the allocator, over core.mem's raw memory and `builtin static` blocks)
/// compiled by compiler/codegen, each `strata_...` function under its C
/// name and with no call-stack frames or cancellation points, as Rust's
/// `llvm::compile_core_rt` makes it. Programs linked with it in place of the
/// C runtime's versions (`STRATA_CORE_RT`: with those compiled out, a
/// missing function fails the link) behave as with the C allocator: the
/// same output, nothing left alive at the end in check mode
/// (`STRATA_MEMORY=check`), through small objects, blocks past the small
/// sizes and tasks (counts changed atomically); and, ending in a fault, the
/// same objects reported alive, which core.rt's own tallies find.
#[test]
fn core_rt_compiled_by_codegen() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ours = Command::new(compiler::exe()).current_dir(root).args(["codegen", "--core-rt"]).output().unwrap();
    let ir = String::from_utf8_lossy(&ours.stdout).into_owned();
    assert!(!ir.lines().any(|l| l.starts_with("error")), "{ir}");
    for f in ["strata_alloc", "strata_dup", "strata_release", "strata_free", "strata_unique", "strata_memory_start", "strata_memory_made"] {
        assert!(ir.contains(&format!("define ")) && ir.lines().any(|l| l.starts_with("define ") && l.contains(&format!(" @{f}("))), "{f} not defined under its C name");
    }
    assert!(!ir.contains("call void @strata_enter") && !ir.contains("call i1 @strata_cancelled"), "core.rt has frames or cancellation points");
    let dir = scratch("core-rt");
    let rt = dir.join("core_rt.ll");
    std::fs::write(&rt, &ir).unwrap();
    // built with core.rt, and with the C runtime's own allocator
    let build = |name: &str, text: &str| -> (PathBuf, PathBuf) {
        let file = dir.join(format!("{name}.strata"));
        save(&file, text);
        let prog = Command::new(compiler::exe()).current_dir(root).arg("codegen").arg(&file).output().unwrap();
        let ll = dir.join(format!("{name}.ll"));
        std::fs::write(&ll, &prog.stdout).unwrap();
        let mut out = vec![];
        for with_rt in [true, false] {
            let exe = dir.join(format!("{name}-{}", if with_rt { "rt" } else { "c" }));
            let mut c = Command::new("clang");
            c.args(["-O2", "-Wno-override-module", "-o"]).arg(&exe).arg(&ll).arg(root.join("src/llvm/runtime.c"));
            if with_rt {
                c.arg("-DSTRATA_CORE_RT").arg(&rt);
            }
            let o = c.arg("-lm").output().unwrap();
            assert!(o.status.success(), "{name}: {}", String::from_utf8_lossy(&o.stderr));
            out.push(exe);
        }
        (out.remove(0), out.remove(0))
    };
    let run = |exe: &PathBuf| Command::new(exe).env("STRATA_MEMORY", "check").output().unwrap();
    // small objects, blocks past the small sizes (a 2000-element list, long
    // strings), and objects made and dropped by tasks
    let (rt_exe, c_exe) = build("work", r#"fn main() {
  xs := [1, 2, 3].map(x -> x * 2)
  big := range(0, 2000).map(n -> n * n)
  long := join(range(0, 300).map(n -> to_string(n)), ",")
  sums := scope {
    a := spawn fold(range(0, 500).map(n -> [n, n + 1]), 0, (s, p) -> s + len(p))
    b := spawn byte_len(join(range(0, 200).map(n -> f"item {n}"), ";"))
    (await a, await b)
  }
  print(f"{xs} {len(big)} {big[1999]} {byte_len(long)} {sums}")
}
"#);
    // check mode's summary, `memory: N allocated, M freed, K live` (core.rt's
    // own tallies: a free it didn't count shows here, and only here)
    let summary = |o: &std::process::Output| -> (u64, u64, u64) {
        let err = String::from_utf8_lossy(&o.stderr).into_owned();
        let line = err.lines().find(|l| l.starts_with("memory: ")).unwrap_or_else(|| panic!("no memory summary: {err}")).to_string();
        let n: Vec<u64> = line.split(|c: char| !c.is_ascii_digit()).filter(|x| !x.is_empty()).map(|x| x.parse().unwrap()).collect();
        (n[0], n[1], n[2])
    };
    let (a, b) = (run(&rt_exe), run(&c_exe));
    assert!(String::from_utf8_lossy(&a.stdout).starts_with("[2, 4, 6] 2000 3996001 1089 (1000, "), "{}", String::from_utf8_lossy(&a.stdout));
    assert_eq!(a.stdout, b.stdout);
    assert!(a.status.success(), "{}", String::from_utf8_lossy(&a.stderr));
    let (made, freed, live_n) = summary(&a);
    assert!(made > 2000 && made == freed && live_n == 0, "core.rt: {made} allocated, {freed} freed, {live_n} live");
    assert_eq!(summary(&a), summary(&b), "core.rt's tallies differ from the C allocator's");
    // a fault: what is still alive is reported, and it is what the C
    // allocator reports (the tallies, and each object's count and size)
    let (rt_exe, c_exe) = build("fault", "fn half(n: Int) -> Int\n  requires n % 2 == 0 {\n  n / 2\n}\n\nfn main() {\n  xs := [4, 2, 3].map(x -> half(x))\n  print(xs)\n}\n");
    let live = |o: &std::process::Output| -> Vec<String> {
        String::from_utf8_lossy(&o.stderr).lines().filter(|l| l.trim_start().starts_with("live:")).map(|l| l.split(", first word").next().unwrap_or("").to_string()).collect()
    };
    let (a, b) = (run(&rt_exe), run(&c_exe));
    assert!(!live(&a).is_empty() && summary(&a).2 > 0, "core.rt reported nothing alive after a fault: {}", String::from_utf8_lossy(&a.stderr));
    assert_eq!(summary(&a), summary(&b));
    assert_eq!(live(&a), live(&b));
    let _ = std::fs::remove_dir_all(&dir);
}

/// core.rt's strings and integers (runtime.c's, moved to Strata): programs
/// built with core.rt (compiled by compiler/codegen) and with the C
/// runtime's versions give the same output, faults (their messages, exit
/// codes, inside a supervised task too) and check mode's report (the same
/// objects made, freed and left alive).
#[test]
fn core_rt_strings_and_integers_as_in_c() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ours = Command::new(compiler::exe()).current_dir(root).args(["codegen", "--core-rt"]).output().unwrap();
    let ir = String::from_utf8_lossy(&ours.stdout).into_owned();
    assert!(!ir.lines().any(|l| l.starts_with("error")), "{ir}");
    let dir = scratch("core-rt-strings");
    let rt = dir.join("core_rt.ll");
    std::fs::write(&rt, &ir).unwrap();
    // each program's output, error output and exit code, with core.rt and with C
    let run = |name: &str, text: &str, sides: &[bool]| -> Vec<String> {
        let file = dir.join(format!("{name}.strata"));
        save(&file, text);
        let prog = Command::new(compiler::exe()).current_dir(root).arg("codegen").arg(&file).output().unwrap();
        let ll = dir.join(format!("{name}.ll"));
        std::fs::write(&ll, &prog.stdout).unwrap();
        let mut outs = vec![];
        for &with_rt in sides {
            let exe = dir.join(format!("{name}-{}", if with_rt { "rt" } else { "c" }));
            let mut c = Command::new("clang");
            c.args(["-O2", "-Wno-override-module", "-o"]).arg(&exe).arg(&ll).arg(root.join("src/llvm/runtime.c"));
            if with_rt {
                c.arg("-DSTRATA_CORE_RT").arg(&rt);
            }
            let o = c.arg("-lm").output().unwrap();
            assert!(o.status.success(), "{name}: {}", String::from_utf8_lossy(&o.stderr));
            let r = Command::new(&exe).env("STRATA_MEMORY", "check").output().unwrap();
            outs.push(format!("{}\n-- stderr\n{}\n-- {:?}", String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&r.stderr), r.status.code()));
        }
        outs
    };
    let both = |name: &str, text: &str| -> String {
        let mut outs = run(name, text, &[true, false]);
        assert_eq!(outs[0], outs[1], "{name}: core.rt (left) and C (right) differ");
        outs.remove(0)
    };
    let out = both("strings", CORE_RT_STRINGS);
    assert!(out.starts_with("héllo, wörld ∑ 😀\n23 true false [\"\", \"ab\", \"abc\", \"b\", \"é\"]\n"), "{out}");
    assert!(out.contains("HÉLLO, WÖRLD ∑ 😀 σας οδος STRASSE\n"), "{out}");
    assert!(out.contains("byte 10 out of range (3 bytes)\n0\nchr: -1 is not a character\nord needs a single character, got \"ab\\n\"\n-1\n"), "{out}");
    assert!(out.contains("memory: ") && out.contains(" 0 live"), "{out}");
    // a fault ending the program: its message, and what was left alive
    for (i, e) in ["byte_at(\"abc\", 3)", "chr(55296)", "ord(\"\\n\\u{1}x\")"].iter().enumerate() {
        let out = both(&format!("fault{i}"), &format!("fn main() {{\n  print(\"start\")\n  x := {e}\n  print(x)\n}}\n"));
        assert!(out.contains("fault (Runtime): ") && out.ends_with("Some(101)"), "{out}");
    }
    let out = both("integers", CORE_RT_INTEGERS);
    assert!(out.starts_with("123456789012345678901234567890 -123456789012345678901234567890 4611686018427387904 -4611686018427387905 15241578753238836750495351562536198787501905199875019052100 0\n"), "{out}");
    // Euclidean: the remainder is never negative
    assert!(out.contains("\n1 4611686018427387904 17636684144620811271604938270 3 -17636684144620811271604938270 4\n-17636684144620811271604938270 3 17636684144620811271604938271 4 1 123456789012345678901234567890\n-4 1 -3 1 0\n"), "{out}");
    assert!(out.contains("\n1180591620717411303424 -36893488147419103232 15432098626543209862654320986 -15432098626543209862654320987 0 -1 11529215046068469760 -13835058055282163712\n"), "{out}");
    assert!(out.contains("\n6148914691236517205 5 4611686018427387904 3 9223372036854775807 18446744073709551616 -9223372036854775808\n"), "{out}");
    assert!(out.contains("division by zero\n0\ndivision by zero\n0\n`<<` by a negative number of bits\n0\n`300` doesn't fit in `U8`\n0\n`-123456789012345678901234567890` doesn't fit in `U8`\n0\n`<<` by a negative number of bits\n0\n"), "{out}");
    // -2^62 / -1 is 2^62, a big integer (C read it as one's address and
    // crashed, until this test)
    let out = both("past_small", "fn main() {\n  x := byte_len(\"\") - 4611686018427387904\n  print(x / -1)\n}\n");
    assert!(out.starts_with("4611686018427387904\n") && out.ends_with("Some(0)"), "{out}");
    for (i, e) in ["1 << -1", "5 / (byte_len(\"\") * 1)", "7 % (byte_len(\"\") * 1)", "1 << 5000000000", "to_u8(-1)", "to_i8(128)"].iter().enumerate() {
        let out = both(&format!("int_fault{i}"), &format!("fn main() {{\n  print(\"start\")\n  x := {e}\n  print(x)\n}}\n"));
        assert!(out.contains("fault (") && out.ends_with("Some(101)"), "{out}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

const CORE_RT_INTEGERS: &str = r#"fn div0(a: Int, b: Int) -> Int {
  a / b
}

fn mod0(a: Int, b: Int) -> Int {
  a % b
}

fn shift(a: Int, n: Int) -> Int {
  a << n
}

fn fit(n: Int) -> U8 {
  to_u8(n)
}

fn word_shift(a: I64, n: I64) -> I64 {
  a << n
}

fn main() {
  big := 123456789012345678901234567890
  small := 4611686018427387903
  print(f"{big} {-big} {small + 1} {-small - 2} {big * big} {big - big}")
  print(f"{big + 1 - big} {(small + 1) * 2 / 2} {(big + 3) / 7} {(big + 3) % 7} {-big / 7} {(-big - 3) % 7}")
  print(f"{(big + 3) / -7} {(big + 3) % -7} {(-big - 3) / -7} {(-big - 3) % -7} {big / big} {big % (big + 1)}")
  print(f"{-7 / 2} {-7 % 2} {7 / -2} {7 % -2} {-4611686018427387904 % -1}")
  print(f"{abs(-big)} {abs(-4611686018427387904)} {big == big + 0} {big < big + 1} {-big < big} {big > small}")
  print(f"{big & 255} {big | 1} {big ^ big} {-big & 65535} {-big | 3} {-big ^ 12345678901234567890123}")
  print(f"{1 << 70} {-1 << 65} {big >> 3} {-big >> 3} {big >> 200} {-big >> 200} {5 << 61} {-3 << 62}")
  print(f"{parse_int("123456789012345678901234567890123")} {parse_int("-1_000")} {parse_int("+0")} {parse_int("1x")}")
  print(f"{to_i64(-9223372036854775808)} {to_u64(18446744073709551615)} {wrap_u64(-1)} {wrap_i64(big)}")
  let x: U64 = to_u64(18446744073709551615)
  let y: I64 = to_i64(-9223372036854775807)
  print(f"{x / to_u64(3)} {x % to_u64(10)} {y / to_i64(-2)} {y % to_i64(10)} {y / to_i64(-1)} {to_int(x) + 1} {to_int(y) - 1}")
  print(f"{float(big)} {float(-small - 2)}")
  scope {
    h := supervised spawn div0(5, 0)
    print(await h except p {
      print(p.message)
      0
    })
    g := supervised spawn mod0(big, 0)
    print(await g except p {
      print(p.message)
      0
    })
    k := supervised spawn shift(3, -1)
    print(await k except p {
      print(p.message)
      0
    })
    m := supervised spawn fit(300)
    print(await m except p {
      print(p.message)
      to_u8(0)
    })
    n := supervised spawn fit(-big)
    print(await n except p {
      print(p.message)
      to_u8(0)
    })
    w := supervised spawn word_shift(to_i64(1), to_i64(-2))
    print(await w except p {
      print(p.message)
      to_i64(0)
    })
  }
}
"#;

const CORE_RT_STRINGS: &str = r#"fn bad_byte(s: String) -> Int {
  byte_at(s, 10)
}

fn bad_chr(n: Int) -> String {
  chr(n)
}

fn bad_ord(s: String) -> Int {
  ord(s)
}

fn main() {
  s := "  héllo, wörld ∑ 😀  "
  t := trim(s)
  print(t)
  print(f"{byte_len(t)} {t == "héllo, wörld ∑ 😀"} {t == "x"} {sort(["b", "abc", "ab", "", "é"])}")
  print(t + "!" + s)
  print(f"{byte_slice(t, 2, 9)} {byte_slice(t, -4, 3)} {byte_slice(t, 5, 2)}")
  print(f"{ord("∑")} {chr(128512)} {chr(233)} {chr(65)} {byte_at(t, 1)}")
  print(debug("a\"b\\n\tc\\"))
  print(f"{to_upper(t)} {to_lower("ΣΑΣ ΟΔΟΣ")} {to_upper("straße")}")
  print(f"{is_digit("123")} {is_alpha("abc1")} {is_alpha("éa")} {is_space(" \t")} {is_digit("")}")
  print(f"{parse_int(" 12_345 ")} {parse_int("x")} {parse_int("-99999999999999999999999")}")
  scope {
    h := supervised spawn bad_byte("abc")
    print(await h except p {
      print(p.message)
      0
    })
    g := supervised spawn bad_chr(-1)
    print(await g except p { p.message })
    k := supervised spawn bad_ord("ab\n")
    print(await k except p {
      print(p.message)
      -1
    })
  }
  print(f"{bad_ord("é")} {bad_chr(97)}")
}
"#;

/// core.list, core.fmt and core.float (runtime.c's lists, format specs
/// and floats, moved to Strata): programs built by each code generator
/// (compiler/codegen, and llvm.rs, whose in-place changes of lists use
/// `strata_list_owned` / `_put` / `_insert` / `_remove_at`), with the
/// runtime in Strata (its own compile of core.rt) and with the C runtime's
/// versions, give the same output, faults and check mode's report.
#[test]
fn core_rt_lists_formats_and_floats_as_in_c() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("core-rt-lists");
    let ours = Command::new(compiler::exe()).current_dir(root).args(["codegen", "--core-rt"]).output().unwrap();
    let ir = String::from_utf8_lossy(&ours.stdout).into_owned();
    assert!(!ir.lines().any(|l| l.starts_with("error")), "{ir}");
    let rt_codegen = dir.join("core_rt_codegen.ll");
    std::fs::write(&rt_codegen, &ir).unwrap();
    let rt_rust = dir.join("core_rt_rust.ll");
    let o = Command::new(STRATA).current_dir(root).arg("build-core").arg("-o").arg(&rt_rust).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // a program's output, error output and exit code, by each code
    // generator, with core.rt and with C: the same
    let both = |name: &str, text: &str| -> String {
        let file = dir.join(format!("{name}.strata"));
        save(&file, text);
        let mut outs = vec![];
        for rust in [false, true] {
            let ll = dir.join(format!("{name}-{}.ll", if rust { "rust" } else { "codegen" }));
            if rust {
                let o = Command::new(STRATA).current_dir(root).args(["build", "--rust", "--no-verify", "--emit-llvm"]).arg(&file).arg("-o").arg(&ll).output().unwrap();
                assert!(o.status.success(), "{name}: {}", String::from_utf8_lossy(&o.stderr));
            } else {
                let prog = Command::new(compiler::exe()).current_dir(root).arg("codegen").arg(&file).output().unwrap();
                std::fs::write(&ll, &prog.stdout).unwrap();
            }
            let mut pair = vec![];
            for with_rt in [true, false] {
                let exe = dir.join(format!("{name}-{rust}-{with_rt}"));
                let mut c = Command::new("clang");
                c.args(["-O2", "-Wno-override-module", "-o"]).arg(&exe).arg(&ll).arg(root.join("src/llvm/runtime.c"));
                if with_rt {
                    c.arg("-DSTRATA_CORE_RT").arg(if rust { &rt_rust } else { &rt_codegen });
                }
                let o = c.arg("-lm").output().unwrap();
                assert!(o.status.success(), "{name}: {}", String::from_utf8_lossy(&o.stderr));
                let r = Command::new(&exe).env("STRATA_MEMORY", "check").output().unwrap();
                pair.push(format!("{}\n-- stderr\n{}\n-- {:?}", String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&r.stderr), r.status.code()));
            }
            assert_eq!(pair[0], pair[1], "{name} ({}): core.rt (left) and C (right) differ", if rust { "llvm.rs" } else { "compiler/codegen" });
            outs.push(pair.remove(0));
        }
        outs.remove(0)
    };
    let out = both("lists", CORE_RT_LISTS);
    assert!(out.starts_with("100 101 1 2 9801\nx y w49 101 true true\n"), "{out}");
    assert!(out.contains("\n32 Some(184) 51 true false\n[1, 3, 5, 9] [\"w0\", \"w1\", \"w10\"]\n"), "{out}");
    let out = both("formats", CORE_RT_FORMATS);
    assert!(out.starts_with("[   42] [42   ] [  42  ] [00042] [-0007] [+42] [-7] [+00042]\n[héllo     ] [     héllo] [  héllo  ] [**héllo**] [hélloééé] [héllo] [🙂🙂🙂héllo]\n"), "{out}");
    let out = both("floats", CORE_RT_FLOATS);
    assert!(out.starts_with("0.1 0.100 0\n1.0 1.000 1\n1e+21 1000000000000000000000.000 1000000000000000000000\n1.5e-7 0.000 0\n"), "{out}");
    assert!(out.contains("\n2.67 0 2 2 -0.00 100000000000000000000.00 123.4560000000 0.000 1.00000000000000000000\n1.5 0.5 1.5 0.5 1.4142135623730951 0.0\n"), "{out}");
    assert!(out.contains("\n2.4703282292062327e-324 -> Some(0.0)\n2.4703282292062328e-324 -> Some(5e-324)\n1.7976931348623158e308 -> Some(1.7976931348623157e+308)\n1.7976931348623159e308 -> None\n"), "{out}");
    assert!(out.contains(" is too large for a Float\n0.0\n") && out.contains(" is not a finite number\n0.0\nsqrt of negative number -2.5\n"), "{out}");
    // floats of every exponent: shown, read back, with 4 decimals
    let out = both("float_bits", CORE_RT_FLOAT_BITS);
    assert!(out.contains("not read back: 0\n"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

const CORE_RT_LISTS: &str = r#"fn main() {
  mut xs := []
  for i in 0..100 {
    xs = push(xs, i * i)
  }
  a := push(xs, 1)
  b := push(xs, 2)
  print(f"{len(xs)} {len(a)} {a[100]} {b[100]} {xs[99]}")
  mut words := []
  for i in 0..50 {
    words = push(words, f"w{i}")
  }
  shared := push(words, "x")
  other := push(words, "y")
  print(f"{shared[50]} {other[50]} {words[49]} {len(words + shared)} {[] + words == words} {words + [] == words}")
  big := [123456789012345678901234567890, 2, -98765432109876543210987654321]
  bigs := big + big
  print(f"{bigs} {push(big, 7)} {big[1..3]} {bigs[2..5]} {range(3, 8)} {range(5, 2)}")
  sl := words[10..20]
  print(f"{sl} {push(sl, "z")} {sl + sl}")
  mut d := Dict[String, Int] {}
  for i in 0..200 {
    d = insert(d, f"k{i % 37}", i)
  }
  mut s := Set[Int] {}
  for i in 0..300 {
    s = insert(s, (i * 7) % 101)
  }
  for i in 0..50 {
    s = remove(s, i)
    d = remove(d, f"k{i % 5}")
  }
  print(f"{len(d)} {get(d, "k36")} {len(s)} {contains(s, 77)} {contains(s, 3)}")
  print(f"{sort([5, 3, 9, 1])} {sort(words)[0..3]}")
}
"#;

const CORE_RT_FLOATS: &str = r#"fn big_float(n: Int) -> Float {
  float(n)
}

fn times(a: Float, b: Float) -> Float {
  a * b
}

fn root(x: Float) -> Float {
  sqrt(x)
}

fn pf(s: String) -> Float {
  parse_float(s) ?? 0.0
}

fn main() {
  xs := [0.1, 1.0, 1000000000000000000000.0, 0.00000015, 123456.789, -0.0, 0.0, 0.3, 1.0 / 3.0, 100.0, 10000000000000000.0, 1000000000000000.0, 12345678901234567.0, 0.000001, 0.0001, -2.5, pf("3.0e-310"), pf("1.7976931348623157e308"), pf("5.0e-324"), pf("2.2250738585072014e-308"), 9007199254740993.0, 0.1 + 0.2]
  for x in xs {
    print(f"{x} {x:.3} {x:.0}")
  }
  print(f"{2.675:.2} {0.5:.0} {1.5:.0} {2.5:.0} {-0.001:.2} {100000000000000000000.0:.2} {123.456:.10} {0.000123:.3} {1.0:.20}")
  print(f"{7.5 % 2.0} {-7.5 % 2.0} {7.5 % -2.0} {-7.5 % -2.0} {sqrt(2.0)} {sqrt(0.0)}")
  print(f"{floor(3.7)} {ceil(-3.2)} {round(2.5)} {floor(100000000000000000000.0)} {round(-123456789012345678901234.0)} {floor(-4611686018427387904.0)}")
  print(f"{float(123456789012345678901234567890)} {float(-98765432109876543210987654321098765)} {float(9007199254740993)} {float(4611686018427387905)}")
  for s in ["1e400", "1e-400", "  -0.0 ", ".5", "5.", "1e", "abc", "2.2250738585072011e-308", "4.9e-324", "2.4703282292062327e-324", "2.4703282292062328e-324", "1.7976931348623158e308", "1.7976931348623159e308", "123456789012345678901234567890e-10", "-1.5E+3", "0.1", "+7", "1_0", "9007199254740993", "1e00000000000000000000001"] {
    print(f"{s} -> {parse_float(s)}")
  }
  print(f"{debug(0.1)} {[1.5, -2.0]} {true} {false}")
  mut huge := 1
  for _ in 0..400 {
    huge = huge * 10
  }
  scope {
    h := supervised spawn big_float(huge)
    print(await h except p {
      print(p.message)
      0.0
    })
    g := supervised spawn times(pf("1.0e300"), pf("1.0e300"))
    print(await g except p {
      print(p.message)
      0.0
    })
    k := supervised spawn root(-2.5)
    print(await k except p {
      print(p.message)
      0.0
    })
  }
}
"#;

const CORE_RT_FLOAT_BITS: &str = r#"fn main() {
  mut seed := 88172645463325252
  mut bad := 0
  for i in 0..3000 {
    seed = (seed * 6364136223846793005 + 1442695040888963407) % 18446744073709551616
    // every exponent, and small ones and near-powers-of-two often
    bits := if i % 3 == 0 {
      seed % 9007199254740992 + (i % 2047) * 4503599627370496
    } else {
      seed
    }
    match float_from_bits(bits) {
      Some(x) => {
        s := to_string(x)
        back := parse_float(s) ?? 0.0
        if back != x {
          bad = bad + 1
        }
        if i % 7 == 0 {
          print(f"{s} {x:.4} {float_bits(back) == float_bits(x)}")
        } else {
          print(s)
        }
      },
      None => print("nan"),
    }
  }
  print(f"not read back: {bad}")
}
"#;

const CORE_RT_FORMATS: &str = r#"fn main() {
  n := 42
  m := -7
  s := "héllo"
  print(f"[{n:5}] [{n:<5}] [{n:^6}] [{n:05}] [{m:05}] [{n:+}] [{m:+}] [{n:+06}]")
  print(f"[{s:10}] [{s:>10}] [{s:^9}] [{s:*^9}] [{s:é<8}] [{s:2}] [{s:🙂>8}]")
  print(f"[{123456789012345678901234567890:40}] [{true:>7}]")
}
"#;

/// `compiler codegen --core-rt` checks the runtime before compiling it: a
/// function of one of its modules that allocates (`Int` arithmetic: two
/// literals added, then compared with a word) is an error, not compiled (Rust's `strata
/// build-core` doesn't check yet).
#[test]
fn core_rt_that_doesnt_check_isnt_compiled() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("core-rt-check");
    // a copy of core/ (and the prelude codegen reads) with one bad function
    let core = dir.join("core");
    std::fs::create_dir_all(&core).unwrap();
    for e in std::fs::read_dir(root.join("core")).unwrap() {
        let e = e.unwrap();
        std::fs::copy(e.path(), core.join(e.file_name())).unwrap();
    }
    std::fs::create_dir_all(dir.join("src/llvm")).unwrap();
    std::fs::copy(root.join("src/llvm/prelude.strata"), dir.join("src/llvm/prelude.strata")).unwrap();
    let text = Command::new(STRATA).current_dir(root).args(["--file", "core", "render", "--rust", "core.gen"]).output().unwrap();
    let text = String::from_utf8_lossy(&text.stdout).into_owned() + "\nfn too_big(x: I64) -> Bool {\n  x < 1023 + 62\n}\n";
    let mut edit = Command::new(STRATA).current_dir(&dir).args(["--file", "core/", "edit", "gen"]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    std::io::Write::write_all(edit.stdin.as_mut().unwrap(), text.as_bytes()).unwrap();
    assert!(edit.wait_with_output().unwrap().status.success());
    let out = Command::new(compiler::exe()).current_dir(&dir).args(["codegen", "--core-rt"]).output().unwrap();
    let out = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.lines().any(|l| l.starts_with("error: in core.gen.too_big: core.rt can't allocate")), "{out}");
    assert!(!out.contains("define "), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// core.gen and core.fault (runtime.c's generators of values and the text
/// of a fault's call stack, moved to Strata): a test's mocked values (built
/// by compiler/codegen, run through `strata_test` as the JIT runs a test)
/// and programs faulting deep in recursions (built by both code
/// generators) do the same with the runtime in Strata and with C: the same
/// values, messages, call stacks (`down (101 times)`, ` -> ...` past the
/// runs kept) and check mode's report.
#[test]
fn core_rt_generators_and_stacks_as_in_c() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("core-rt-gen");
    let ours = Command::new(compiler::exe()).current_dir(root).args(["codegen", "--core-rt"]).output().unwrap();
    let ir = String::from_utf8_lossy(&ours.stdout).into_owned();
    assert!(!ir.lines().any(|l| l.starts_with("error")), "{ir}");
    let rt_codegen = dir.join("core_rt_codegen.ll");
    std::fs::write(&rt_codegen, &ir).unwrap();
    let rt_rust = dir.join("core_rt_rust.ll");
    let o = Command::new(STRATA).current_dir(root).arg("build-core").arg("-o").arg(&rt_rust).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // a test run as the JIT runs one (`strata_test`), from a C `main`
    let harness = dir.join("harness.c");
    std::fs::write(&harness, HARNESS).unwrap();
    // IR (`ll`) with C's runtime, plus `extra` files, with core.rt (`rt`) and
    // without: their output, error output and exit code, the same
    let run_both = |name: &str, ll: &Path, rt: &Path, extra: &[&Path]| -> String {
        let mut pair = vec![];
        for with_rt in [true, false] {
            let exe = dir.join(format!("{name}-{with_rt}"));
            let mut c = Command::new("clang");
            c.args(["-O2", "-Wno-override-module", "-o"]).arg(&exe).arg(ll).args(extra).arg(root.join("src/llvm/runtime.c"));
            if with_rt {
                c.arg("-DSTRATA_CORE_RT").arg(rt);
            }
            let o = c.arg("-lm").output().unwrap();
            assert!(o.status.success(), "{name}: {}", String::from_utf8_lossy(&o.stderr));
            let r = Command::new(&exe).env("STRATA_MEMORY", "check").output().unwrap();
            pair.push(format!("{}\n-- stderr\n{}\n-- {:?}", String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&r.stderr), r.status.code()));
        }
        assert_eq!(pair[0], pair[1], "{name}: core.rt (left) and C (right) differ");
        pair.remove(0)
    };
    // mocked values, in a test
    let file = dir.join("mocks.strata");
    save(&file, CORE_RT_MOCKS);
    let tests = Command::new(compiler::exe()).current_dir(root).arg("codegen").arg("--tests").arg(&file).output().unwrap();
    let ll = dir.join("mocks.ll");
    std::fs::write(&ll, &tests.stdout).unwrap();
    let out = run_both("mocks", &ll, &rt_codegen, &[&harness]);
    assert!(out.contains("-- 0\n") && out.lines().filter(|l| l.contains("Item {")).count() >= 30, "{out}");
    // faults deep in recursions, by both code generators
    for (name, text, stack) in [("stack", CORE_RT_STACK, "    stack: main -> two -> two -> two -> down (101 times)\n"), ("stack_cut", CORE_RT_STACK_CUT, " -> ping -> pong -> ping -> ...\n")] {
        let file = dir.join(format!("{name}.strata"));
        save(&file, text);
        for rust in [false, true] {
            let ll = dir.join(format!("{name}-{rust}.ll"));
            if rust {
                let o = Command::new(STRATA).current_dir(root).args(["build", "--rust", "--no-verify", "--emit-llvm"]).arg(&file).arg("-o").arg(&ll).output().unwrap();
                assert!(o.status.success(), "{name}: {}", String::from_utf8_lossy(&o.stderr));
            } else {
                let prog = Command::new(compiler::exe()).current_dir(root).arg("codegen").arg(&file).output().unwrap();
                std::fs::write(&ll, &prog.stdout).unwrap();
            }
            let out = run_both(&format!("{name}-{rust}"), &ll, if rust { &rt_rust } else { &rt_codegen }, &[]);
            assert!(out.contains("fault (Runtime): byte 5 out of range (2 bytes)\n") && out.contains(stack) && out.ends_with("Some(101)"), "{out}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

const HARNESS: &str = r##"#include <stdio.h>
#include <stdint.h>
typedef struct { int64_t len; char bytes[]; } Str;
extern void strata_init(int32_t, char **);
extern int32_t strata_test(void (*)(void));
extern const char *strata_test_printed(void), *strata_test_kind(void), *strata_test_stack(void);
extern Str *strata_test_message(void);
#ifdef __APPLE__
extern void test0(void) __asm__("_strata.test.0");
#else
extern void test0(void) __asm__("strata.test.0");
#endif
int main(int argc, char **argv) {
  strata_init(argc, argv);
  int32_t r = strata_test(test0);
  printf("%s-- %d\n", strata_test_printed(), r);
  if (r) { Str *m = strata_test_message(); printf("%s: %.*s [%s]\n", strata_test_kind(), (int)m->len, m->bytes, strata_test_stack()); }
  return 0;
}
"##;

const CORE_RT_MOCKS: &str = r#"type Score = Int where 0 <= it && it <= 100

type Item = { name: String, qty: Int where it > 0 }

type Mood = Happy | Sad | Calm

trait Store {
  fn price(self, sku: String) -> Int
    ensures it >= 0
  fn score(self) -> Score
  fn items(self, n: Int) -> List[Item]
    requires n >= 0
  fn pick(self) -> Option[Bool]
  fn ratio(self) -> Float
  fn mood(self) -> Mood
  fn tag(self) -> String
}

test "mocked values" {
  s := mock Store
  for i in 0..40 {
    print(f"{s.price("a")} {s.score()} {s.items(3)} {s.pick()} {s.ratio()} {s.mood()} {s.tag()}")
  }
  assert s.score() <= 100
}
"#;

const CORE_RT_STACK: &str = r#"fn down(n: Int) -> Int {
  if n == 0 {
    return byte_at("ab", 5)
  }
  down(n - 1) + 1
}

fn two(n: Int) -> Int {
  if n == 0 {
    down(100)
  } else {
    two(n - 1)
  }
}

fn main() {
  print("start")
  print(two(2))
}
"#;

const CORE_RT_STACK_CUT: &str = r#"fn ping(n: Int) -> Int {
  if n == 0 {
    return byte_at("ab", 5)
  }
  pong(n - 1) + 1
}

fn pong(n: Int) -> Int {
  ping(n) + 1
}

fn main() {
  print(ping(2100))
}
"#;

/// core.fault and core.run (runtime.c's call stack, faults, tests' and fuzz
/// cases' runs and `--trace`, moved to Strata over `builtin thread static`
/// state and core.mem's `trap_run` / `trap_leave`): a test that faults
/// (what it printed kept, the fault's message and stack) and a fuzz case
/// (run through `strata_test` / `strata_fuzz` from a C `main`), a traced
/// program, and tasks that fault (supervised: caught; not: ending the
/// program), built by compiler/codegen (and llvm.rs for the tasks), do the
/// same with the runtime in Strata and with C.
#[test]
fn core_rt_runs_faults_and_traces_as_in_c() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("core-rt-runs");
    let ours = Command::new(compiler::exe()).current_dir(root).args(["codegen", "--core-rt"]).output().unwrap();
    let ir = String::from_utf8_lossy(&ours.stdout).into_owned();
    assert!(!ir.lines().any(|l| l.starts_with("error")), "{ir}");
    assert!(ir.contains("thread_local global"), "the runtime's per-thread state isn't thread-local");
    let rt_codegen = dir.join("core_rt_codegen.ll");
    std::fs::write(&rt_codegen, &ir).unwrap();
    let rt_rust = dir.join("core_rt_rust.ll");
    let o = Command::new(STRATA).current_dir(root).arg("build-core").arg("-o").arg(&rt_rust).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let harness = dir.join("harness.c");
    std::fs::write(&harness, RUN_HARNESS).unwrap();
    let fuzz_harness = dir.join("harness_fuzz.c");
    std::fs::write(&fuzz_harness, RUN_HARNESS.replace("strata_test(test0)", "strata_fuzz(test0, 5, 1000)")).unwrap();
    let run_both = |name: &str, ll: &Path, rt: &Path, extra: &[&Path]| -> String {
        let mut pair = vec![];
        for with_rt in [true, false] {
            let exe = dir.join(format!("{name}-{with_rt}"));
            let mut c = Command::new("clang");
            c.args(["-O2", "-Wno-override-module", "-o"]).arg(&exe).arg(ll).args(extra).arg(root.join("src/llvm/runtime.c"));
            if with_rt {
                c.arg("-DSTRATA_CORE_RT").arg(rt);
            }
            let o = c.arg("-lm").output().unwrap();
            assert!(o.status.success(), "{name}: {}", String::from_utf8_lossy(&o.stderr));
            let r = Command::new(&exe).env("STRATA_MEMORY", "check").output().unwrap();
            pair.push(format!("{}\n-- stderr\n{}\n-- {:?}", String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&r.stderr), r.status.code()));
        }
        assert_eq!(pair[0], pair[1], "{name}: core.rt (left) and C (right) differ");
        pair.remove(0)
    };
    let codegen = |name: &str, text: &str, flag: Option<&str>| -> PathBuf {
        let file = dir.join(format!("{name}.strata"));
        save(&file, text);
        let mut c = Command::new(compiler::exe());
        c.current_dir(root).arg("codegen");
        if let Some(f) = flag {
            c.arg(f);
        }
        let o = c.arg(&file).output().unwrap();
        let ll = dir.join(format!("{name}.ll"));
        std::fs::write(&ll, &o.stdout).unwrap();
        ll
    };
    // a test that faults: what it printed before, the fault and its stack
    let ll = codegen("tfault", RUNS_TEST_FAULT, Some("--tests"));
    for (name, h) in [("tfault", &harness), ("tfault-fuzz", &fuzz_harness)] {
        let out = run_both(name, &ll, &rt_codegen, &[h.as_path()]);
        assert!(out.starts_with("before\n2\n-- 1\nRequires: precondition of `half` violated: requires n % 2 == 0 (with n = 3) [deep (8 times) -> half]\n"), "{out}");
    }
    // `--trace`
    let ll = codegen("trace", RUNS_TRACE, Some("--trace"));
    let out = run_both("trace", &ll, &rt_codegen, &[]);
    assert!(out.contains("\u{2192} main()\n  \u{2192} total([1, 2, 3])\n") && out.contains("\u{2190} total: 6\n"), "{out}");
    // tasks: a supervised one's fault caught, another's ending the program
    let ll = codegen("tasks", RUNS_TASKS, None);
    let out = run_both("tasks", &ll, &rt_codegen, &[]);
    assert!(out.starts_with("Runtime: byte 5 out of range (1 bytes) [main -> spawn -> bad]\n0\n1\n") && out.contains("fault (Runtime): byte 7 out of range (1 bytes)\n    stack: main -> spawn -> bad\n") && out.ends_with("Some(101)"), "{out}");
    let file = dir.join("tasks.strata");
    let ll = dir.join("tasks-rust.ll");
    let o = Command::new(STRATA).current_dir(root).args(["build", "--rust", "--no-verify", "--emit-llvm"]).arg(&file).arg("-o").arg(&ll).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(run_both("tasks-rust", &ll, &rt_rust, &[]), out);
    let _ = std::fs::remove_dir_all(&dir);
}

const RUN_HARNESS: &str = r##"#include <stdio.h>
#include <stdint.h>
typedef struct { int64_t len; char bytes[]; } Str;
extern void strata_init(int32_t, char **);
extern int32_t strata_test(void (*)(void));
extern int32_t strata_fuzz(void (*)(void), int64_t, int64_t);
extern const char *strata_test_printed(void), *strata_test_kind(void), *strata_test_stack(void);
extern Str *strata_test_message(void);
#ifdef __APPLE__
extern void test0(void) __asm__("_strata.test.0");
#else
extern void test0(void) __asm__("strata.test.0");
#endif
int main(int argc, char **argv) {
  strata_init(argc, argv);
  int32_t r = strata_test(test0);
  printf("%s-- %d\n", strata_test_printed(), r);
  if (r) { Str *m = strata_test_message(); printf("%s: %.*s [%s]\n", strata_test_kind(), (int)m->len, m->bytes, strata_test_stack()); }
  return 0;
}
"##;

const RUNS_TEST_FAULT: &str = r#"fn half(n: Int) -> Int
  requires n % 2 == 0 {
  n / 2
}

fn deep(n: Int) -> Int {
  if n == 0 {
    half(3)
  } else {
    deep(n - 1) + 1
  }
}

test "a fault ends the test, what it printed kept" {
  print("before")
  print(f"{half(4)}")
  x := deep(7)
  print("never")
}
"#;

const RUNS_TRACE: &str = r#"fn add(a: Int, b: Int) -> Int {
  a + b
}

fn total(xs: List[Int]) -> Int {
  if len(xs) == 0 {
    0
  } else {
    add(xs[0], total(xs[1..len(xs)]))
  }
}

fn main() {
  print(total([1, 2, 3]))
}
"#;

const RUNS_TASKS: &str = r#"fn bad(n: Int) -> Int {
  if n > 2 {
    byte_at("x", n)
  } else {
    n
  }
}

fn main() {
  scope {
    h := supervised spawn bad(5)
    print(await h except p {
      print(f"{p.kind}: {p.message} [{p.stack}]")
      0
    })
    g := spawn bad(1)
    print(await g)
  }
  scope {
    k := spawn bad(7)
    print(await k)
  }
}
"#;

/// `strata build --compare`: compiler/codegen's executable does what Rust's
/// does on the build tests' programs and the example packages with a `main`
/// (the measure for switching `build` over).
#[test]
fn build_compare_agrees_on_the_programs_and_examples() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut targets: Vec<PathBuf> = ["data", "lists", "numbers", "sized"].iter().map(|n| root.join(format!("tests/data/build/{n}.strata"))).collect();
    targets.extend(["hello", "fuzzy", "wc", "page"].iter().map(|n| root.join("examples").join(n)));
    for t in targets {
        let (dir, arg) = if t.is_dir() { (t.clone(), ".".to_string()) } else { (root.to_path_buf(), t.to_string_lossy().into_owned()) };
        let o = Command::new(STRATA).current_dir(&dir).arg("build").arg(&arg).arg("--compare").env("STRATA_COMPILER", root.join("compiler")).output().unwrap();
        let out = String::from_utf8_lossy(&o.stdout);
        assert!(out.contains("compare: they agree"), "{}: {out}{}", t.display(), String::from_utf8_lossy(&o.stderr));
    }
}

/// `compiler jit`: compiler/codegen's IR run in the compiler's own process
/// by LLVM's JIT (compiler/c/jit.c) does what Rust's executable does: its
/// output and its exit code.
#[test]
fn jit_runs_programs_as_built_ones_run() {
    let dir = scratch("jit");
    let chosen: Vec<_> = PROGRAMS.iter().filter(|(n, _)| ["arithmetic", "iterators", "tasks_and_channels", "regex_matches"].contains(n)).collect();
    assert_eq!(chosen.len(), 4);
    for (name, text) in chosen {
        let file = dir.join(format!("{name}.strata"));
        save(&file, text);
        let rust = dir.join(format!("{name}-rust"));
        let o = Command::new(STRATA).arg("build").arg(&file).arg("-o").arg(&rust).output().unwrap();
        assert!(o.status.success(), "{name}: strata build: {}", String::from_utf8_lossy(&o.stderr));
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let ours = Command::new(compiler::exe()).current_dir(root).arg("jit").arg(&file).output().unwrap();
        let theirs = Command::new(&rust).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&theirs.stdout), String::from_utf8_lossy(&ours.stdout), "{name}");
        assert_eq!(theirs.status.code(), ours.status.code(), "{name}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `run --strata --jit` with std's precompiled library loaded into the
/// compiler's process: a program's calls of std (`examples/hello`'s
/// `std.cli`) reach the library's code, not the compiler executable's own
/// copies of the same functions (compiled by Rust, laid out differently:
/// `no match arm for Some(...)` in `std.cli.where_of` before).
#[test]
fn jit_uses_the_std_library_not_the_compilers_copies() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let hello = root.join("examples/hello");
    for args in [vec!["Ann"], vec!["--times", "2", "--loud", "Bo"], vec!["--times", "11", "Bo"]] {
        let run = |strata: bool| {
            let mut c = Command::new(STRATA);
            c.current_dir(&hello).arg("run");
            if strata {
                c.args(["--strata", "--jit"]);
            }
            let o = c.arg(".").arg("--").args(&args).env("STRATA_COMPILER", root.join("compiler")).output().unwrap();
            (String::from_utf8_lossy(&o.stdout).into_owned(), o.status.code())
        };
        assert_eq!(run(false), run(true), "{args:?}");
    }
}

/// `--trace` through compiler/codegen (`compiler jit --trace`, which `strata
/// run --strata --jit --trace` runs): each call and return on stderr, its
/// values shown by the compiled code, as `strata run --trace` shows them.
#[test]
fn jit_traces_as_rust_traces() {
    let dir = scratch("trace");
    let file = dir.join("traced.strata");
    save(&file, "type Point = { x: Int, y: Int }\n\nfn total(xs: List[Int]) -> Int {\n    fold(xs, 0, (a, b) -> a + b)\n}\n\nfn shift(p: Point, d: Int) -> Point {\n    Point { x: p.x + d, y: p.y }\n}\n\nfn greet(name: String) {\n    print(\"hi \" + name)\n}\n\nfn fact(n: Int) -> Int {\n    if n <= 1 {\n        1\n    } else {\n        n * fact(n - 1)\n    }\n}\n\nfn main() {\n    print(total([1, 2, 3]))\n    print(shift(Point { x: 1, y: 2 }, 3))\n    greet(\"ann\")\n    print(fact(3))\n}\n");
    let rust = Command::new(STRATA).args(["run", "--rust", "--trace"]).arg(&file).output().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ours = Command::new(compiler::exe()).current_dir(root).args(["jit", "--trace"]).arg(&file).output().unwrap();
    assert!(String::from_utf8_lossy(&ours.stderr).contains("    → fact(1)\n"), "{}", String::from_utf8_lossy(&ours.stderr));
    assert_eq!(String::from_utf8_lossy(&rust.stderr), String::from_utf8_lossy(&ours.stderr));
    assert_eq!(String::from_utf8_lossy(&rust.stdout), String::from_utf8_lossy(&ours.stdout));
    let _ = std::fs::remove_dir_all(&dir);
}

/// std precompiled once (plan item C21): `compiler codegen --library` holds
/// std's functions that don't change per program (`; precompiled NAME`
/// lines, each an exported symbol); a program compiled with `--precompiled
/// NAMES` declares them instead, and linked with the library's object it
/// does what the program compiled whole does (and frees everything).
#[test]
fn std_precompiled_once_links_with_programs() {
    if !have_clang() {
        return;
    }
    let dir = scratch("stdlib");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ir = compiler::run(&["codegen", "--library"]);
    assert!(!ir.lines().any(|l| l.starts_with("error")), "{}", ir.lines().filter(|l| l.starts_with("error")).collect::<Vec<_>>().join("\n"));
    let names: Vec<&str> = ir.lines().filter_map(|l| l.strip_prefix("; precompiled ")).collect();
    assert!(names.len() > 100 && names.contains(&"std.json.parse"), "{names:?}");
    std::fs::write(dir.join("std.ll"), &ir).unwrap();
    std::fs::write(dir.join("names"), names.join("\n")).unwrap();
    let o = Command::new(cc()).args(["-O2", "-fPIC", "-Wno-override-module", "-c", "-o"]).arg(dir.join("std.o")).arg(dir.join("std.ll")).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let runtime = root.join("src/llvm/runtime.c");
    for ex in ["wc", "page"] {
        let pkg = root.join("examples").join(ex);
        let whole = compiler::run(&["codegen".to_string(), pkg.to_string_lossy().into_owned()]);
        let part = compiler::run(&["codegen".to_string(), "--precompiled".to_string(), dir.join("names").to_string_lossy().into_owned(), pkg.to_string_lossy().into_owned()]);
        assert!(!part.starts_with("error") && part.len() < whole.len(), "{ex}: {}", &part[..part.len().min(400)]);
        let mut outs = vec![];
        for (k, text) in [("whole", &whole), ("part", &part)] {
            let (ll, exe) = (dir.join(format!("{ex}-{k}.ll")), dir.join(format!("{ex}-{k}")));
            std::fs::write(&ll, text).unwrap();
            let mut c = Command::new(cc());
            c.args(["-O2", "-Wno-override-module", "-o"]).arg(&exe).arg(&ll).arg(&runtime);
            if k == "part" {
                c.arg(dir.join("std.o"));
            }
            let o = c.arg("-lm").output().unwrap();
            assert!(o.status.success(), "{ex} {k}: {}", String::from_utf8_lossy(&o.stderr));
            let run = Command::new(&exe).current_dir(&pkg).env("STRATA_MEMORY", "check").output().unwrap();
            let err = String::from_utf8_lossy(&run.stderr).into_owned();
            assert!(err.contains(" 0 live"), "{ex} {k}: {err}");
            outs.push((String::from_utf8_lossy(&run.stdout).into_owned(), run.status.code()));
        }
        assert_eq!(outs[0], outs[1], "{ex}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `compiler fuzz`: compiler/codegen's drivers (`codegen --fuzz-text`) run
/// in the compiler's own process by its JIT (compiler/c/jit.c), each case's
/// arguments handed back as code: the verdict of each function is the one
/// `strata verify --fuzz` gets from Rust's drivers (cases, discards, the
/// counterexample and its message). A function compiler/codegen can't
/// compile yet is left out of the comparison.
#[test]
fn compiler_fuzzes_as_verify_fuzzes() {
    let dir = scratch("compiler-fuzz");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![];
    for (name, text) in [
        ("step1", include_str!("data/smt/step1.st")),
        ("step2", include_str!("data/smt/step2.st")),
        ("step3", include_str!("data/smt/step3.st")),
        ("step4", include_str!("data/smt/step4.st")),
        ("termination", include_str!("data/smt/termination.st")),
    ] {
        let file = dir.join(format!("{name}.strata"));
        save(&file, text);
        files.push(file);
    }
    files.push(root.join("examples/math.strata"));
    let lines = |out: &[u8]| -> Vec<(String, Vec<String>)> {
        let mut fns: Vec<(String, Vec<String>)> = vec![];
        for l in String::from_utf8_lossy(out).lines() {
            if let Some(n) = l.strip_prefix("fn ") {
                fns.push((n.to_string(), vec![]));
            } else if let (Some(x), Some(f)) = (l.strip_prefix("  "), fns.last_mut()) {
                f.1.push(x.to_string());
            }
        }
        fns
    };
    let mut compared = 0;
    for f in files {
        let rust = Command::new(STRATA).arg("--file").arg(&f).args(["verify", "--fuzz"]).output().unwrap();
        assert!(rust.status.success(), "{}: {}", f.display(), String::from_utf8_lossy(&rust.stderr));
        let ours = Command::new(compiler::exe()).current_dir(root).arg("fuzz").arg(&f).output().unwrap();
        assert!(ours.status.success(), "{}: {}", f.display(), String::from_utf8_lossy(&ours.stderr));
        let rust = lines(&rust.stdout);
        for (n, verdict) in lines(&ours.stdout) {
            if verdict.first().is_some_and(|v| v.starts_with("skipped: it can't be compiled yet")) {
                continue;
            }
            let r = rust.iter().find(|x| x.0 == n).unwrap_or_else(|| panic!("{}: `{n}` is not among Rust's", f.display()));
            assert_eq!(r.1, verdict, "{}: `{n}`: Rust's verdict, then compiler fuzz's", f.display());
            compared += 1;
        }
    }
    assert!(compared > 50, "only {compared} functions compared");
}

