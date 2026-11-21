//! `strata build`: a compiled program prints what the interpreter prints,
//! faults included, and exits with the same code.

use std::path::{Path, PathBuf};
use std::io::Write;
use std::process::{Command, Stdio};


/// Store a program written as text at `path` (a module file): text only goes
/// in through an edit.
fn save(path: &std::path::Path, src: impl AsRef<str>) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_strata"))
        .args(["--file", &path.to_string_lossy(), "edit"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(src.as_ref().as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success(), "{}: {}", path.display(), String::from_utf8_lossy(&o.stderr));
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("strata-build-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn have_clang() -> bool {
    let cc = std::env::var("STRATA_CC").unwrap_or_else(|_| "clang".into());
    Command::new(cc).arg("--version").output().is_ok_and(|o| o.status.success())
}

/// Output (stdout then stderr) and exit code.
fn output(c: &mut Command) -> (String, i32) {
    let o = c.output().unwrap();
    // the interpreter also prints the call stack of a fault
    let err: String = String::from_utf8_lossy(&o.stderr).lines().filter(|l| !l.starts_with("    stack:")).map(|l| format!("{l}\n")).collect();
    (String::from_utf8_lossy(&o.stdout).to_string() + &err, o.status.code().unwrap_or(-1))
}

/// The arguments every program is run with.
const ARGS: &[&str] = &["one", "two words"];

/// Build `file` natively, run it, and run it in the interpreter: same output.
fn same_as_interpreter(file: &Path, dir: &Path) -> (String, i32) {
    let strata = env!("CARGO_BIN_EXE_strata");
    let exe = dir.join("prog");
    let b = Command::new(strata).arg("build").arg(file).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    // both run in `dir`, with the same arguments (what they read and write stays there)
    let native = freed_everything(output(Command::new(&exe).args(ARGS).current_dir(dir).env("STRATA_MEMORY", "check")), file);
    let interp = output(Command::new(strata).args(["run", "--interpret"]).arg(file).arg("--").args(ARGS).current_dir(dir));
    assert_eq!(native, interp, "native (left) and interpreted (right) runs differ for {}", file.display());
    native
}

/// A compiled program's output without its memory report (`STRATA_MEMORY`),
/// which must say it freed everything it made, unless a fault stopped it.
fn freed_everything((out, code): (String, i32), file: &Path) -> (String, i32) {
    // the report, then the objects still alive (in check mode)
    let report = out.lines().find(|l| l.starts_with("memory: ")).unwrap_or("").to_string();
    let rest: String = out.lines().filter(|l| !l.starts_with("memory: ") && !l.starts_with("  live: ")).map(|l| format!("{l}\n")).collect();
    assert!(!report.contains("of a freed object"), "{}: {report}\n{out}", file.display());
    if code != 101 {
        assert!(report.ends_with(" 0 live"), "{}: {report}\n{out}", file.display());
    }
    (rest, code)
}

fn case(dir: &Path, name: &str, src: &str) -> (String, i32) {
    let f = dir.join(format!("{name}.strata"));
    save(&f, src);
    same_as_interpreter(&f, dir)
}

#[test]
fn numbers_compile_and_run_as_interpreted() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("numbers");
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/build/numbers.strata");
    let (out, code) = same_as_interpreter(&file, &d);
    assert!(out.starts_with("2432902008176640000\n"), "{out}");
    assert!(out.contains("fault (Refinement): result of `next_digit`"), "{out}");
    assert_eq!(code, 101);
}

#[test]
fn faults_and_control_flow_match_the_interpreter() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("faults");
    let cases = [
        ("early_return", "fn sign(n: Int) -> Int { if n < 0 { return -1 } else { 0 } }\nfn main() -> () { print(sign(-5)) print(sign(5)) }", 0),
        ("requires", "fn half(n: Int) -> Int requires n % 2 == 0 { n / 2 }\nfn main() -> () { print(half(4)) print(half(3)) }", 101),
        ("ensures", "fn f(x: Int) -> Int ensures it > x { x }\nfn main() -> () { print(f(2)) }", 101),
        ("assert", "fn main() { s := \"a\\\"b\" assert s + \"!\" == \"x\" }", 101),
        ("assignment", "fn main() { mut x: Int where it < 5 = 0 while true { x = x + 1 } }", 101),
        ("division", "fn main() { print(-7 / 2) print(-7 % 2) print(7 / -2) print(1 / 0) }", 101),
        ("floats", "fn main() { print(0.1 + 0.2) print(1000000000000000000000.0) print(0.00000015) print(100.0) print(-5.5 % 2.0) print(1.0 / 0.0) }", 101),
        ("exit_code", "fn main() -> Int { 3 }", 3),
        (
            "loops",
            "fn g(n: Int) -> Int { mut i := 0 while i < 10 { i = i + 1 if i == n { break } if i % 2 == 0 { continue } } i }\n\
             fn pick(b: Bool) -> String { match b { true => \"yes\", false => return \"no\" } }\n\
             fn main() { print(g(4)) print(g(40)) print(pick(true) + pick(false)) print(f\"{1.5} {true} {\"q\"} {len(\"héllo\")}\") }",
            0,
        ),
    ];
    for (name, src, want) in cases {
        let (out, code) = case(&d, name, src);
        assert_eq!(code, want, "{name}: {out}");
    }
}

#[test]
fn records_variants_and_generics_run_as_interpreted() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("data");
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/build/data.strata");
    let (out, code) = same_as_interpreter(&file, &d);
    assert!(out.contains("Err(\"3 is odd\")") && out.contains("big circle, circle, rect or dot, nothing"), "{out}");
    assert_eq!(code, 101);
    let cases = [
        ("field_refinement", "type Quantity = Int where 1 <= it && it <= 99\ntype Line = { sku: String, qty: Quantity }\nfn main() -> () { mut l := Line { sku: \"a\", qty: 5 } l.qty = 0 }", 101),
        (
            "tree",
            "type Tree[T] =\n  | Leaf\n  | Node(Tree[T], T, Tree[T])\n\
             fn insert(t: Tree[Int], x: Int) -> Tree[Int] { match t { Leaf => Node(Leaf, x, Leaf), Node(l, v, r) => if x < v { Node(insert(l, x), v, r) } else { Node(l, v, insert(r, x)) } } }\n\
             fn size[T](t: Tree[T]) -> Int { match t { Leaf => 0, Node(l, _, r) => size(l) + 1 + size(r) } }\n\
             fn main() { mut t: Tree[Int] = Leaf for i in range(0, 10) { t = insert(t, (i * 7) % 10) } print(size(t)) print(insert(insert(Leaf, 2), 1)) }",
            0,
        ),
        ("own_methods", "type Item = { name: String, price: Int }\nimpl Item {\n  fn new(name: String) -> Item { Item { name: name, price: 0 } }\n  fn label(self) -> String { f\"{self.name}: {self.price}\" }\n}\nfn main() -> () { print(Item.new(\"cup\").label()) }", 0),
        ("refined_argument", "fn f(o: Option[Positive]) -> Int { o ?? 0 }\nfn main() -> () { print(f(Some(3))) let x: Option[Int] = Some(-2) print(f(x)) }", 101),
        ("main_err", "fn main() -> Result[(), String] { print(\"working\") Err(\"it broke\") }", 1),
        (
            "errors",
            "type E =\n  | NotFound\n  | Bad { why: String }\n\
             fn g(n: Int) -> Result[Int, E] { if n == 0 { Err(NotFound) } else if n < 0 { Err(Bad { why: \"neg\" }) } else { Ok(n) } }\n\
             fn h(n: Int) -> Result[Int, E] { v := g(n)! Ok(v * 2) }\n\
             fn main() { print(h(3)) print(h(0)) print(h(-1)) print(h(0) == Err(NotFound)) (x, y) := (5, \"s\") print(f\"{x}{y}\") }",
            0,
        ),
    ];
    for (name, src, want) in cases {
        let (out, code) = case(&d, name, src);
        assert_eq!(code, want, "{name}: {out}");
    }
}

#[test]
fn lists_and_lambdas_run_as_interpreted() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("lists");
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/build/lists.strata");
    let (out, code) = same_as_interpreter(&file, &d);
    assert!(out.contains("[2, 3, 5, 7, 11, 13, 17, 19, 23, 29]") && out.contains("[0]: -1 does not satisfy `Positive`"), "{out}");
    assert_eq!(code, 101);
    // examples/math.strata, with a `main` that prints what its test checks
    let math = Command::new(env!("CARGO_BIN_EXE_strata")).args(["render", "--file"]).arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/math.strata")).output().unwrap();
    let math = String::from_utf8(math.stdout).unwrap();
    let math = math[..math.find("test \"values\"").unwrap()].to_string()
        + "fn main() {\n  print([factorial(5), gcd(12, 18), power(2, 10), digit_sum(1234), ackermann(2, 3), triangle(10), isqrt(17), collatz_steps(6)])\n  print(positive_squares([-2, 3, 0, 4]))\n  print(primes(20))\n  print(first_or_one([5, 1]))\n  print(average([]))\n}\n";
    let (out, code) = case(&d, "math", &math);
    assert!(out.contains("[120, 6, 1024, 10, 9, 55, 4, 8]") && out.contains("division by zero"), "{out}");
    assert_eq!(code, 101);
    // no limit on a range's length (the interpreter's was 1,000,000)
    let (out, code) = case(&d, "range", "fn main() {\n  mut n: Int = 0\n  for i in range(0, 2000000) { n = n + i }\n  print(n)\n  print(len(range(0, 1500000)))\n}");
    assert_eq!((out.as_str(), code), ("1999999000000\n1500000\n", 0));
}

#[test]
fn traits_run_as_interpreted() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("traits");
    // examples/traits.strata, its tests as `main` (without the regex of `Sku`)
    let src = tests_as_main("examples/traits.strata", "");
    let (out, code) = case(&d, "traits_example", &src);
    assert!(out.contains("all asserts hold") && code == 0, "{out}");
    let cases = [
        ("trait_ensures", "trait Describe {\n  fn describe(self) -> String\n    ensures len(it) > 0\n}\ntype Thing = { name: String }\nimpl Describe for Thing {\n  fn describe(self) -> String { self.name }\n}\nfn main() -> () { print(Thing { name: \"a\" }.describe()) print(Thing { name: \"\" }.describe()) }", 101),
        ("variant_impls", "trait Area {\n  fn area(self) -> Int\n    requires true\n}\ntype Shape =\n  | Square { side: Int }\n  | Circle { r: Int }\nimpl Area for Shape {\n  fn area(self) -> Int { 0 }\n}\nimpl Area for Square {\n  fn area(self) -> Int { self.side * self.side }\n}\nfn main() -> () { let shapes: List[Shape] = [Square { side: 3 }, Circle { r: 2 }] print(shapes.map(s -> s.area())) }", 0),
        ("generic_traits", "trait Convert[T] {\n  fn convert(self) -> T\n}\ntype Celsius = { deg: Int }\nimpl Convert[String] for Celsius {\n  fn convert(self) -> String { f\"{self.deg}C\" }\n}\nimpl Convert[Int] for Int {\n  fn convert(self) -> Int { self * 2 }\n}\nfn twice[A: Convert[String]](xs: List[A]) -> List[String] { xs.map(x -> x.convert()) }\nfn main() -> () { print(Celsius { deg: 20 }.convert()) print(twice([Celsius { deg: 1 }])) print(21.convert()) }", 0),
        ("default_methods", "trait Shown {\n  fn shown(self) -> String\n  fn twice(self) -> String { self.shown() + self.shown() }\n}\nimpl Shown for Bool { fn shown(self) -> String { if self { \"yes\" } else { \"no\" } } }\nimpl Shown for String { fn shown(self) -> String { self } }\nfn all_shown[T: Shown](xs: List[T]) -> String { join(xs.map(x -> x.twice()), \" \") }\nfn main() -> () { print(all_shown([true, false])) print(all_shown([\"a\", \"b\"])) }", 0),
        ("method_type_params", "trait Boxed {\n  fn wrap[U](self, other: U) -> (Self, U)\n}\ntype K = { k: Int }\nimpl Boxed for K { fn wrap[U](self, other: U) -> (Self, U) { (self, other) } }\nfn main() -> () { print(K { k: 1 }.wrap(\"x\")) print(K { k: 2 }.wrap(3.5)) }", 0),
    ];
    for (name, src, want) in cases {
        let (out, code) = case(&d, name, src);
        assert_eq!(code, want, "{name}: {out}");
    }
}

#[test]
fn capabilities_and_mut_run_as_interpreted() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("capabilities");
    let cases = [
        ("console", "use std.io.Console\nfn greet(mut console: Console, name: String) -> () { mut console.print(f\"hello, {name}\") }\nfn main(mut console: Console) -> () { greet(mut console, \"world\") mut console.eprint(\"to stderr\") mut console.write(\"no newline\") mut console.print(\"\") }"),
        ("mut_params", "fn restock(mut n: Int, extra: Int) { n = n + extra }\ntype Line = { sku: String, qty: Int }\nfn bump(mut l: Line) -> () { l.qty = l.qty + 1 }\nfn main() -> () { mut k := 1 restock(mut k, 5) print(k) mut line := Line { sku: \"a\", qty: 1 } bump(mut line) bump(mut line) print(line) restock(mut line.qty, 10) print(line) }"),
        ("files_and_environment", "use std.io.Console\nuse std.fs.{Files, FsError}\nuse std.process.Environment\nfn main(mut console: Console, mut files: Files, env: Environment) -> Result[(), String] {\n  _ := mut files.remove(\"fsplay/b.txt\")\n  _ := mut files.remove(\"fsplay/a.txt\")\n  _ := mut files.remove(\"fsplay/sub/deep\")\n  _ := mut files.remove(\"fsplay/sub\")\n  _ := mut files.remove(\"fsplay\")\n  print(files.exists(\"fsplay\"))\n  print(mut files.make_dir(\"fsplay/sub/deep\"))\n  print(mut files.write(\"fsplay/a.txt\", \"one\\n\"))\n  print(mut files.append(\"fsplay/a.txt\", \"two\\n\"))\n  print(files.read(\"fsplay/a.txt\"))\n  print(files.read(\"fsplay/missing.txt\"))\n  print(files.read(\"fsplay/sub\"))\n  print(mut files.write(\"fsplay/b.txt\", \"x\"))\n  print(files.list(\"fsplay\"))\n  print(files.list(\"fsplay/a.txt\"))\n  print(mut files.remove(\"fsplay/nope\"))\n  print(env.args())\n  print(env.var(\"HOME\") is Some)\n  print(env.var(\"STRATA_SURELY_UNSET_VAR\"))\n  mut console.print(\"done\")\n  Ok(())\n}"),
        ("one_function_two_consoles", "use std.io.{Console, recorder}\nfn report(mut console: Console, n: Int) -> () {\n  for i in range(0, n) { mut console.print(f\"line {i}\") }\n}\nfn main(mut console: Console) -> () {\n  mut r := recorder([])\n  report(mut r, 3)\n  report(mut console, 2)\n  print(r.out)\n  print(r)\n}"),
    ];
    for (name, src) in cases {
        let (out, code) = case(&d, name, src);
        assert_eq!(code, 0, "{name}: {out}");
    }
    // the files test made and read a folder of its own, there
    assert!(d.join("fsplay/a.txt").exists());
    let (out, _) = case(&d, "args", "use std.process.Environment\nfn main(env: Environment) -> () { print(env.args()) }");
    assert!(out.contains(r#"["one", "two words"]"#), "{out}");
}

/// A program's text (rendered), its tests turned into blocks of a `main`.
fn tests_as_main(file: &str, skip: &str) -> String {
    let src = Command::new(env!("CARGO_BIN_EXE_strata")).args(["render", "--file"]).arg(Path::new(env!("CARGO_MANIFEST_DIR")).join(file)).output().unwrap();
    let src = String::from_utf8(src.stdout).unwrap();
    let mut decls = String::new();
    let mut main = String::from("fn main() {\n");
    let mut in_test = false;
    for line in src.lines() {
        if line.starts_with("test ") {
            in_test = true;
            main += "  {\n";
        } else if in_test && line == "}" {
            in_test = false;
            main += "  }\n";
        } else if in_test {
            if skip.is_empty() || !line.contains(skip) {
                main += line;
                main += "\n";
            }
        } else {
            decls += line;
            decls += "\n";
        }
    }
    main += "  print(\"all asserts hold\")\n}\n";
    decls + &main
}

#[test]
fn strings_dicts_and_sets_run_as_interpreted() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("collections");
    // examples/builtins.strata, its tests as `main`
    let (out, code) = case(&d, "builtins_example", &tests_as_main("examples/builtins.strata", ""));
    assert!(out.contains("all asserts hold") && code == 0, "{out}");
    // examples/wc: its functions, with stand-ins for the console and the files (not its options)
    let wc = Command::new(env!("CARGO_BIN_EXE_strata")).args(["render", "--file"]).arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/wc/main.strata")).output().unwrap();
    let wc = String::from_utf8(wc.stdout).unwrap();
    let wc = wc[..wc.find("test \"").unwrap()].replace("fn main(", "fn wc(") + WC_MAIN;
    let (out, code) = case(&d, "wc", &wc);
    assert!(out.contains(r#"["2\t3\t14\ta.txt", "1\t2\t12\tb.txt", "3\t5\t26\ttotal"]"#) && out.contains("NotFound") && code == 0, "{out}");
    let (out, code) = case(&d, "strings", STRINGS);
    assert!(out.contains(r#"["STRASSE Ǆ ʼN", "οδος σ ας. i̇", "SS", "ǆ"]"#) && out.contains("[-00042]") && code == 0, "{out}");
    let (out, code) = case(&d, "dicts_and_sets", COLLECTIONS);
    assert!(out.contains(r#"Dict { "cat": 2, "end": 1, "hat": 1, "the": 3 }"#) && out.contains("Dict { 1: \"a\", 2: \"b\", 3: \"z\" }") && code == 0, "{out}");
    let faults = [
        ("dict_value", "fn f() -> Dict[String, Positive] { Dict { \"a\": 1, \"b\": 0 } }\nfn main() -> () { print(f()) }"),
        ("set_element", "fn g(s: Set[Positive]) -> Int { len(s) }\nfn main() -> () { print(g(Set { 1 })) print(g(Set { 3, -2 })) }"),
        ("dict_key", "fn main() { let d: Dict[NonEmptyString, Int] = Dict { \"x\": 1, \"\": 2 } print(d) }"),
        ("set_assigned", "fn main() { mut s: Set[Natural] = Set { 1 } s = insert(s, -1) }"),
        ("split", "fn main() { print(split(\"abc\", \"\")) }"),
        ("replace", "fn main() { print(replace(\"abc\", \"\", \"x\")) }"),
        ("repeat", "fn main() { print(repeat(\"ab\", 3)) print(repeat(\"ab\", 60000000)) }"),
        ("ord", "fn main() { print(ord(\"a\")) print(ord(\"a\\\"\\n\")) }"),
        ("chr", "fn main() { print(chr(55296)) }"),
        ("string_index", "fn main() { print(\"abc\"[2]) print(\"abc\"[5]) }"),
        ("list_index", "fn main() { print([1, 2][-1]) }"),
    ];
    for (name, src) in faults {
        let (out, code) = case(&d, name, src);
        assert!(code == 101 && out.contains("fault ("), "{name}: {out}");
    }
}

const WC_MAIN: &str = r#"fn main() {
  files := mem_files([("a.txt", "one two\nthree\n"), ("b.txt", "four  five" + chr(13) + "\n")])
  for (only, names) in [(None, ["a.txt", "b.txt"]), (Some(Words), ["a.txt"]), (Some(Chars), ["b.txt", "a.txt"]), (None, ["nope.txt"])] {
    mut console := recorder([])
    r := wc(Options { only: only, files: names }, mut console, files)
    print(r)
    print(printed(console))
  }
  mut console := recorder(["hello world", "bye"])
  print(wc(Options { only: None, files: [] }, mut console, mem_files([])))
  print(printed(console))
  print(count("a b  c\n\nd"))
}
"#;

const STRINGS: &str = r#"fn main() {
  cr := chr(13)
  text := "one two" + cr + "\nthree\n\nfour" + cr
  print(lines(text))
  print([lines(""), lines("\n"), lines("a\n"), lines("a\n\nb")])
  print(split("a,,b,", ","))
  print(split("", ","))
  print(split("héllo wörld", "ö"))
  print(trim(chr(160) + " \t x y " + chr(12288) + "\n"))
  print([contains("héllo", "ll"), contains("a", ""), starts_with("Zoë", "Zo"), ends_with("Zoë", "ë"), ends_with("a", "ab")])
  print(reverse("Zoë!"))
  print(chars("añb€😀"))
  print([get("Zoë", 2), get("Zoë", 3), get("Zoë", -1)])
  print([slice("héllo", 1, 3), slice("héllo", -5, 2), slice("héllo", 3, 1), slice("héllo", 2, 100)])
  print([index_of("añb€", "€"), index_of("abc", "z"), index_of("abc", "")])
  print(replace("a-b-c--", "-", "+=") + "|" + replace("aaa", "aa", "b"))
  print(repeat("ab", 3) + "|" + repeat("x", 0) + "|")
  print([to_upper("straße ǆ ŉ"), to_lower("ΟΔΟΣ Σ ΑΣ. İ"), to_upper("ß"), to_lower("ǅ")])
  print([is_digit("0123"), is_digit(""), is_digit("1a"), is_digit("٣"), is_alpha("añ日本"), is_alpha("a1"), is_space(" \t\n"), is_space(chr(8239)), is_space("")])
  print([ord("A"), ord("€"), ord("😀")])
  print([chr(65), chr(8364), chr(128512)])
  print([parse_int("42"), parse_int(" -7 "), parse_int("+3"), parse_int("1_000_"), parse_int("x"), parse_int("9223372036854775807"), parse_int("-9223372036854775808")])
  print([parse_float("2.5"), parse_float("-1e-3"), parse_float("5."), parse_float(".5"), parse_float("1e"), parse_float("inf"), parse_float("12")])
  x := 3.14159
  n := -42
  s := "héllo"
  print(f"[{x:.2}] [{x:8.3}] [{n:>6}] [{n:<6}] [{n:^7}] [{n:06}] [{n:+}] [{7:+05}] [{s:.2}] [{s:>8}] [{s:*^9}] [{s:3}] [{0.125:.2}] [{true:>6}] [{-0.0:.1}]")
  w := "abc"
  print([w[0], w[2], w[1..], w[..2], w[..]])
  xs := [10, 20, 30, 40]
  print([xs[1], xs[3]])
  print([xs[1..3], xs[2..], xs[..1], xs[3..1], xs[-2..10]])
  print([to_json("a\"b"), to_json([1, 2]), to_json(Some("x"))])
}
"#;

const COLLECTIONS: &str = r#"type Color =
  | Red
  | Green
  | Blue

distinct type UserId = Int

type Point = { x: Int, y: Int }

fn tally(words: List[String]) -> Dict[String, Int] {
  mut d: Dict[String, Int] = Dict {}
  for w in words {
    d = insert(d, w, (get(d, w) ?? 0) + 1)
  }
  d
}

fn main() {
  d := Dict { "b": 2, "a": 1, "c": 3 }
  print(d)
  print([len(d), len(Dict[Int, Int] {})])
  print(Dict[Int, String] {})
  print(dict([(3, "c"), (1, "a"), (3, "z"), (2, "b")]))
  print(insert(d, "a", 10))
  print(insert(d, "aa", 5))
  print(remove(d, "b"))
  print(remove(d, "zz"))
  print([get(d, "c"), get(d, "q")])
  print([contains(d, "a"), contains(d, "x")])
  print(keys(d))
  print(values(d))
  print(items(d))
  print(map_values(d, v -> float(v) * 1.5))
  print(map_values(d, v -> f"<{v}>"))
  for (k, v) in d {
    print(f"{k}={v}")
  }
  print(tally(split("the cat the hat the end cat", " ")))
  s := Set { 3, 1, 3, 2 }
  print(s)
  print([len(s), len(Set[String] {})])
  print([contains(s, 2), contains(s, 7)])
  print([insert(s, 0), insert(s, 2), remove(s, 1), remove(s, 9)])
  print(to_list(s))
  print([union(s, Set { 5, 0, 2 }), intersection(s, Set { 2, 3, 9 }), difference(s, Set { 2 })])
  for x in s {
    print(x)
  }
  print(s == Set { 1, 2, 3 })
  print(Dict { 1: "a" } == dict([(1, "a")]))
  print(d != remove(d, "a"))
  print(set(["pear", "apple", "fig", "apple"]))
  print(set([2.5, -1.0, 0.0, -0.5]))
  print(set([Blue, Red, Blue, Green]))
  print(set([UserId(10), UserId(9), UserId(100)]))
  print(set([Point { x: 2, y: 1 }, Point { x: 1, y: 5 }, Point { x: 2, y: 1 }]))
  print(set([(2, "b"), (1, "z"), (2, "a")]))
  print(set([[2, 1], [1], [1, 0]]))
  print(set([Some(3), None, Some(1)]))
  print(Dict { Red: 1, Blue: 2 })
  print(Dict { (1, 2): "p", (0, 9): "q" })
  print(Set { Set { 2, 1 }, Set { 0 } })
  print(Dict { "x": Set { 1 }, "y": Set { } })
  nested := Dict { "k": Dict { 2: "two", 1: "one" } }
  print(nested)
  print(f"{d} {s}")
  print(to_string(s) + to_json(d))
  print([d][0])
}
"#;

#[test]
fn trait_values_run_as_interpreted() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("dyn");
    let cases = [
        ("trait_values_in_lists", "trait Shape {\n  fn area(self) -> Int\n  fn name(self) -> String {\n    \"shape\"\n  }\n}\n\ntype Square = { side: Int }\n\ntype Rect = { w: Int, h: Int }\n\nimpl Shape for Square {\n  fn area(self) -> Int {\n    self.side * self.side\n  }\n\n  fn name(self) -> String {\n    \"square\"\n  }\n}\n\nimpl Shape for Rect {\n  fn area(self) -> Int {\n    self.w * self.h\n  }\n}\n\nfn total(shapes: List[Shape]) -> Int {\n  sum(shapes.map(s -> s.area()))\n}\n\nfn main() -> () {\n  let shapes: List[Shape] = [Square { side: 2 }, Rect { w: 2, h: 5 }]\n  print(total(shapes))\n  print(shapes.map(s -> s.name()))\n  print(shapes)\n  let one: Shape = Square { side: 3 }\n  print(one.area())\n  print(one)\n}", 0),
        ("trait_values_in_fields", "use std.io.{Console, recorder}\n\ntype Worker = { console: Console, count: Int }\n\nfn work(mut w: Worker) -> () {\n  mut w.console.print(f\"job {w.count}\")\n  w.count = w.count + 1\n}\n\nfn main(console: Console) -> () {\n  mut real := Worker { console, count: 1 }\n  work(mut real)\n  work(mut real)\n  mut fake := Worker { console: recorder([]), count: 7 }\n  work(mut fake)\n  print(fake.count)\n}", 0),
        ("trait_values_are_values", "use std.io.{Console, recorder}\n\ntype Worker = { console: Console, count: Int }\n\nfn work(mut w: Worker) -> () {\n  mut w.console.print(f\"job {w.count}\")\n}\n\nfn main() -> () {\n  mut a := Worker { console: recorder([]), count: 1 }\n  mut b := a\n  work(mut a)\n  work(mut a)\n  work(mut b)\n  print(a.console)\n  print(b.console)\n  print(a)\n}", 0),
        ("mut_proven_fits", "fn grow(mut n: Positive) {\n  n = n + 1\n}\n\nfn main() -> () {\n  mut p: Natural = 5\n  grow(mut p)\n  print(p)\n}", 0),
        ("mut_field_breaks_invariant", "fn grow(mut n: Int) {\n  n = n + 1\n}\n\ntype R = { lo: Int, hi: Int }\n  where it.lo <= it.hi\n\nfn main() -> () {\n  mut r := R { lo: 1, hi: 2 }\n  grow(mut r.lo)\n  grow(mut r.lo)\n  print(r)\n}", 101),
    ];
    for (name, src, want) in cases {
        let (out, code) = case(&d, name, src);
        assert_eq!(code, want, "{name}: {out}");
    }
}

#[test]
fn derived_traits_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("derived");
    let (out, code) = case(&d, "derived", r#"use std.typing.{Record, Field}

/// Values from text, field by field.
derivable(Record) trait FromText {
  /// The fields' names.
  derived fn names(self) -> List[String] {
    comptime $Self.fields.map(f -> f.name)
  }
  /// A value with every field at its zero.
  derived fn zero() -> Self {
    $Self.make(f -> zero_of(f))
  }
  /// A value read from one text per field.
  derived fn parse(texts: List[String]) -> Result[Self, String] {
    $Self.try_make(f -> read(f, texts))
  }
}

fn zero_of(comptime f: Field) -> f.type {
  match comptime f.type {
    Int => 0,
    String => "",
    Bool => false,
  }
}

fn read(comptime f: Field, texts: List[String]) -> Result[f.type, String] {
  text := get(texts, index_of_name(comptime f.name)) or { "" }
  match comptime f.type {
    Int => match parse_int(text) {
      Some(n) => Ok(n),
      None => Err(f"{f.name}: `{text}` is not a number"),
    },
    String => Ok(text),
    Bool => Ok(text == "yes"),
  }
}

fn index_of_name(name: String) -> Int {
  if name == "x" {
    0
  } else if name == "label" {
    1
  } else {
    2
  }
}

type Point = { x: Int, label: String, on: Bool }

derive FromText for Point

fn main() {
  print(Point.zero().names())
  print(Point.zero())
  print(Point.parse(["3", "here", "yes"]))
  print(Point.parse(["three", "here", "yes"]))
}
"#);
    assert!(out.contains("is not a number") && code == 0, "{out}");
}

#[test]
fn derived_for_sums_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("derived-sums");
    // `comptime for`, a match comprehension, `v.make()`
    let (out, code) = case(&d, "sums", r#"use std.typing.{Choice, Sum, Field}

/// A choice and its name.
derivable(Choice) trait Named {
  derived fn from_name(name: String) -> Option[Self] {
    comptime for v in $Self.variants {
      if to_lower(v.name) == name {
        return Some(v.make())
      }
    }
    None
  }
  derived fn name(self) -> String {
    match self {
      v => to_lower(v.name) for v in $Self.variants
    }
  }
}

/// How many fields each variant has.
derivable(Sum) trait Width
  requires len($Self.variants) > 0 {
  derived fn width(self) -> Int {
    match self {
      v => len(v.fields) for v in $Self.variants
    }
  }
}

type Level =
  | Quiet
  | Loud

derive Named for Level

type Shape =
  | Dot
  | Rect(Int, Int)
  | Circle { r: Int, label: String }

derive Width for Shape

fn main() {
  print(Level.from_name("loud"))
  print(Level.from_name("nope"))
  print(Loud.name())
  print([Dot, Rect(1, 2), Circle { r: 1, label: "c" }].map(s -> s.width()))
}
"#);
    assert!(out.contains("Some(Loud)\nNone\nloud\n[0, 2, 2]") && code == 0, "{out}");
}

/// The argument sets the examples with options are run with: values,
/// defaults, `--help`, and bad arguments (exit code 2).
const OPTION_RUNS: &[(&str, &[&str])] = &[
    ("hello", &[]),
    ("hello", &["Ada", "--loud"]),
    ("hello", &["--times", "2", "Ada", "Bob"]),
    ("hello", &["--times=3"]),
    ("hello", &["--times", "30"]),
    ("hello", &["--times", "x"]),
    ("hello", &["--help"]),
    ("hello", &["-h"]),
    ("hello", &["--bogus"]),
    ("wc", &["--help"]),
    ("wc", &["--only", "lines", "examples/wc/strata.toml", "README.md"]),
    ("wc", &["--only", "bytes"]),
    ("wc", &["examples/wc/strata.toml"]),
];

/// `main(opts: Options)`: a compiled program reads its options with the
/// derived `from_args`, as `strata run` does (same output, same exit code).
#[test]
fn options_from_the_command_line_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("options");
    let strata = env!("CARGO_BIN_EXE_strata");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // named after the package, as `strata build` names it (`--help` shows the name)
    for ex in ["hello", "wc"] {
        let b = Command::new(strata).arg("build").arg(root.join("examples").join(ex)).arg("-o").arg(d.join(ex)).output().unwrap();
        assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    }
    let mut codes = vec![];
    for (ex, args) in OPTION_RUNS {
        let native = output(Command::new(d.join(ex)).args(*args).current_dir(root).stdin(Stdio::null()));
        let interp = output(Command::new(strata).args(["run", "--interpret"]).arg(root.join("examples").join(ex)).arg("--").args(*args).current_dir(root).stdin(Stdio::null()));
        assert_eq!(native, interp, "native (left) and interpreted (right) differ for {ex} {args:?}");
        codes.push(native.1);
    }
    assert_eq!(codes, [0, 0, 0, 0, 2, 2, 0, 0, 2, 0, 0, 2, 0]);
    // a mutable options parameter, in a script
    let f = d.join("opts.strata");
    save(&f, "use std.cli.CommandLine\n\ntype Options = {\n  /// How many.\n  count: Option[Int],\n  names: List[String],\n}\n\nderive CommandLine for Options\n\nfn main(mut opts: Options) {\n  opts.names = opts.names + [\"end\"]\n  print(f\"{opts.count} {opts.names}\")\n}\n");
    let exe = d.join("opts");
    let b = Command::new(strata).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    for args in [&["--count", "4", "a", "b"][..], &[], &["--count"], &["--help"]] {
        let native = output(Command::new(&exe).args(args));
        let interp = output(Command::new(strata).args(["run", "--interpret"]).arg(&f).arg("--").args(args));
        assert_eq!(native, interp, "native (left) and interpreted (right) differ for {args:?}");
    }
}

#[test]
fn what_is_not_compiled_yet_is_named() {
    let d = tmp("unsupported");
    let f = d.join("many.strata");
    save(&f, "trait Many {\n  fn copies(self) -> List[Self]\n}\n\ntype A = { n: Int }\n\nimpl Many for A {\n  fn copies(self) -> List[A] { [self, self] }\n}\n\nfn main() {\n  let m: Many = A { n: 1 }\n  print(len(m.copies()))\n}\n");
    let o = Command::new(env!("CARGO_BIN_EXE_strata")).arg("build").arg(&f).arg("--emit-llvm").output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    let why = "can't compile `main`: calling `copies` (it mentions `Self` inside another type) on a value of the trait type `Many` is not compiled yet";
    assert!(!o.status.success() && err.contains(why), "{err}");
    // `strata run` says so too (nothing is interpreted instead)
    let o = Command::new(env!("CARGO_BIN_EXE_strata")).arg("run").arg(&f).output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success() && err.contains(&format!("the program can't be compiled yet: {why}")), "{err}");
}

#[test]
fn emits_llvm_ir() {
    let d = tmp("emit");
    let f = d.join("p.strata");
    save(&f, "fn twice(n: Int) -> Int { n * 2 }\nfn main() -> () { print(twice(21)) }\n");
    let o = Command::new(env!("CARGO_BIN_EXE_strata")).arg("build").arg(&f).arg("--emit-llvm").output().unwrap();
    let ir = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(ir.contains("define i64 @\"strata:twice\"(i64 %p.n)") && ir.contains("define i32 @main(i32 %argc, ptr %argv)"), "{ir}");
}

/// `strata run --jit`: compiled in memory, same output as interpreted.
#[test]
fn the_jit_runs_as_the_interpreter() {
    let d = tmp("jit");
    let strata = env!("CARGO_BIN_EXE_strata");
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/build/numbers.strata");
    let jit = output(Command::new(strata).args(["run", "--jit"]).arg(&file));
    let interp = output(Command::new(strata).args(["run", "--interpret"]).arg(&file));
    assert_eq!(jit, interp);
    assert_eq!(jit.1, 101);
    // a pattern built at run time: Rust's engine, given to the runtime (as the interpreter's)
    let f = d.join("lists.strata");
    save(&f, "fn main() { p := \"[a-z]\" + \"[0-9]\" print(matches(\"a1\", p)) print(matches(\"a\", p + \"(\")) }\n");
    let jit = output(Command::new(strata).args(["run", "--jit"]).arg(&f));
    assert_eq!(jit, output(Command::new(strata).args(["run", "--interpret"]).arg(&f)));
    assert!(jit.0.starts_with("true\nfault (Runtime): bad regex: regex parse error:") && jit.1 == 101, "{jit:?}");
    // a built program has no engine for them, and says so
    let exe = d.join("lists");
    assert!(Command::new(strata).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap().status.success());
    let (out, code) = output(&mut Command::new(&exe));
    assert!(code == 101 && out.contains("a pattern built at run time needs `strata run`"), "{out}");
}

/// `strata run --jit` with options: the derived `from_args`, compiled.
#[test]
fn the_jit_reads_options() {
    let strata = env!("CARGO_BIN_EXE_strata");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (ex, args) in OPTION_RUNS {
        let run = |jit: bool| output(Command::new(strata).arg("run").arg(if jit { "--jit" } else { "--interpret" }).arg(root.join("examples").join(ex)).arg("--").args(*args).current_dir(root).stdin(Stdio::null()));
        let (jit, interp) = (run(true), run(false));
        assert!(!jit.0.contains("note: interpreted"), "{}", jit.0);
        assert_eq!(jit, interp, "the JIT (left) and the interpreter (right) differ for {ex} {args:?}");
    }
}

/// Every compiled program frees what it made (`same_as_interpreter` checks
/// it): one that goes through closures, trait values, `mut`, patterns,
/// early exits, collections and strings, and a chain too long to drop
/// recursively.
#[test]
fn memory_is_freed() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("memory");
    let (out, code) = case(&d, "features", MEMORY);
    assert!(code == 0 && out.contains("Named { name: \"new\", tags: [\"t\", \"renamed\"] }"), "{out}");
    // an operand that borrows a variable a later operand assigns
    let (out, code) = case(&d, "nested_assign", NESTED_ASSIGN);
    assert!(code == 0 && out.starts_with("first 1/1\nsecond 2\n"), "{out}");
    let chain = "type Chain = | End | Link(Int, Chain)\nfn main() { mut c: Chain = End mut i := 0 while i < 1000000 { c = Link(i, c) i += 1 } match c { Link(x, _) => print(x), End => print(0) } }\n";
    // natively only: the interpreter takes minutes on this loop
    let f = d.join("chain.strata");
    save(&f, chain);
    let exe = d.join("chain");
    let b = Command::new(env!("CARGO_BIN_EXE_strata")).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
    let (out, code) = freed_everything(output(Command::new(&exe).env("STRATA_MEMORY", "check")), &f);
    assert!(code == 0 && out == "999999\n", "{out}");
}

const MEMORY: &str = r#"type Shape =
  | Square { side: Int }
  | Circle { r: Int }

trait Area {
  fn area(self) -> Int
  fn grow(mut self)
}

impl Area for Shape {
  fn area(self) -> Int {
    match self {
      Square { side } => side * side,
      Circle { r } => 3 * r * r,
    }
  }
  fn grow(mut self) {
    self = match self {
      Square { side } => Square { side: side + 1 },
      Circle { r } => Circle { r: r + 1 },
    }
  }
}

type Named = { name: String, tags: List[String] }

type Box = { shape: Area, label: String }

fn first_long(xs: List[String]) -> Option[String] {
  for x in xs {
    if len(x) > 3 {
      return Some(x)
    }
  }
  None
}

fn parse_all(xs: List[String]) -> Result[List[Int], String] {
  mut out := []
  for x in xs {
    n := parse_int(x) or { return Err(f"bad {x}") }
    out = push(out, n)
  }
  Ok(out)
}

fn total(xs: List[String]) -> Result[Int, String] {
  ns := parse_all(xs)!
  Ok(ns.fold(0, (a, b) -> a + b))
}

fn rename(mut n: Named, to: String) {
  n.name = to
  n.tags = push(n.tags, "renamed")
}

fn describe(xs: List[Int]) -> String {
  match xs {
    [] => "empty",
    [x] => f"one {x}",
    [1, b, ..rest] => f"one then {b} then {rest}",
    [a, ..rest] => f"starts {a}, {len(rest)} more",
  }
}

fn pick(s: Shape) -> String {
  match s {
    Square { side: 1 } | Circle { r: 1 } => "unit",
    Square { side } | Circle { r: side } => f"size {side}",
  }
}

fn main() {
  // closures capturing strings and lists
  prefix := "item-"
  names := ["a", "bb", "cccc", "ddddd"]
  labelled := names.map(n -> prefix + n)
  print(labelled)
  print(labelled.filter(n -> len(n) > 6))
  adder := (x: Int) -> x + len(prefix)
  print(adder(1))
  // early return inside a loop, Option
  print(first_long(names))
  print(first_long(["a"]))
  // ? / ! and `or { return }`
  print(total(["1", "2", "3"]))
  print(total(["1", "x"]))
  // mut parameters, fields
  mut n := Named { name: "old", tags: ["t"] }
  rename(mut n, "new")
  print(n)
  mut holder := Named { name: "h", tags: [] }
  holder.tags = n.tags
  holder.name = "inner"
  print(holder)
  print(n)
  // trait values in a list and a field, with a mut self call
  let shapes: List[Area] = [Square { side: 2 }, Circle { r: 1 }]
  print(shapes.map(s -> s.area()))
  mut b := Box { shape: Square { side: 3 }, label: "b" }
  mut b.shape.grow()
  print(b.shape.area())
  mut sh: Area = Circle { r: 2 }
  mut sh.grow()
  print(sh.area())
  // list patterns, or-patterns
  print(describe([]))
  print(describe([7]))
  print(describe([1, 2, 3, 4]))
  print(describe([5, 1, 0]))
  print(pick(Square { side: 1 }))
  print(pick(Circle { r: 4 }))
  // loops with break / continue and locals
  mut acc := []
  mut i := 0
  while true {
    i += 1
    s := f"n{i}"
    if i % 2 == 0 {
      continue
    }
    if i > 7 {
      break
    }
    acc = push(acc, s)
  }
  print(acc)
  for w in names {
    t := w + "!"
    if len(w) == 2 {
      continue
    }
    if len(w) == 5 {
      break
    }
    print(t)
  }
  // dicts and sets
  mut d := Dict { "a": 1 }
  d = insert(d, "b", 2)
  d = insert(d, "a", 3)
  print(d)
  print(get(d, "a"))
  print(keys(d))
  s := Set { "x", "y" }
  print(insert(s, "z"))
  print(contains(s, "x"))
  // strings
  text := "  Hello, World  "
  print(trim(text).to_upper())
  print(split("a,b,c", ","))
  print(replace("aXbXc", "X", "--"))
  print(f"{3.14159:.2} [{42:>6}] [{"ab":<4}]")
  print(chars("héllo").reverse())
  print(parse_float(" 2.5 "))
  print(lines("one\ntwo"))
  // reassigning in a loop
  mut words := []
  for k in 0..5 {
    words = words + [f"w{k}"]
  }
  print(words)
  print(words[1..3])
  print(words.sort_by(w -> w).reverse())
}
"#;

/// With the JIT, `strata run` compiles by default: what doesn't compile is
/// interpreted without a note (`--jit` gives one); compiled and interpreted,
/// `Int` grows into a big integer.
#[test]
fn run_compiles_by_default_with_the_jit() {
    let d = tmp("jit-default");
    let strata = env!("CARGO_BIN_EXE_strata");
    let f = d.join("big.strata");
    save(&f, "fn main() {\n  mut n := 1\n  for _ in 0..70 {\n    n = n * 2\n  }\n  print(n)\n}\n");
    let (out, code) = output(Command::new(strata).arg("run").arg(&f));
    assert_eq!((out.as_str(), code), ("1180591620717411303424\n", 0), "compiled");
    let (out, code) = output(Command::new(strata).args(["run", "--interpret"]).arg(&f));
    assert_eq!((out.as_str(), code), ("1180591620717411303424\n", 0));
    let f = d.join("regex.strata");
    save(&f, "fn main() { print(matches(\"a1\", \"[a-z][0-9]\")) }\n");
    assert_eq!(output(Command::new(strata).arg("run").arg(&f)), ("true\n".to_string(), 0));
}

const NESTED_ASSIGN: &str = r#"type P = { name: String, n: Int }

fn show2(a: String, b: Int) -> String { f"{a}/{b}" }

fn pop(mut xs: List[String]) -> String {
  x := xs[len(xs) - 1]
  xs = xs[..len(xs) - 1]
  x
}

fn main() {
  mut s := f"first {1}"
  print(show2(s, { s = f"second {2}"; 1 }))
  print(s)
  mut xs := [f"a{1}", f"b{2}"]
  print([xs, { xs = [f"c{3}"]; [] }])
  print(xs[0] + { xs = []; "!" })
  mut p := P { name: f"p{1}", n: 1 }
  q := P { ..p, n: { p = P { name: f"z{9}", n: 9 }; 2 } }
  print(q)
  mut ws := [f"w{1}", f"w{2}"]
  print((ws, mut ws.pop()))
  print(ws)
}
"#;

/// Fields of unions of records (at different places in each), assigning
/// tuple fields, `==` on trait values, any pattern in a `for` over a
/// range; and the call stack a fault shows, as the interpreter's.
#[test]
fn fields_trait_equality_and_stacks_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("small-gaps");
    let (out, code) = case(&d, "fields", FIELDS);
    assert!(code == 0 && out.contains("Ci { name: \"renamed\", r: 2 }\n") && out.contains("((1, 9), \"x\")"), "{out}");
    let (out, code) = case(&d, "trait_equality", TRAIT_EQUALITY);
    assert!(code == 0 && out.starts_with("true\ntrue\ntrue\ntrue\n") && out.ends_with("zero\nzero\n"), "{out}");
    // the stack, which `output` leaves out
    let f = d.join("stack.strata");
    save(&f, STACK);
    let exe = d.join("stack");
    let strata = env!("CARGO_BIN_EXE_strata");
    let b = Command::new(strata).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
    let native = Command::new(&exe).output().unwrap();
    let interp = Command::new(strata).arg("run").arg(&f).output().unwrap();
    let err = String::from_utf8_lossy(&native.stderr).to_string();
    assert_eq!(err, String::from_utf8_lossy(&interp.stderr));
    assert!(err.contains("    stack: main -> Thing.describe -> inner -> half\n"), "{err}");
    // running out of stack is a fault, not a crash
    let (out, code) = {
        let f = d.join("deep.strata");
        save(&f, "fn deep(n: Int) -> Int { if n == 0 { 0 } else { 1 + deep(n - 1) } }\nfn main() { print(deep(100000000)) }\n");
        let exe = d.join("deep");
        assert!(Command::new(strata).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap().status.success());
        output(&mut Command::new(&exe))
    };
    assert!(code == 101 && out.contains("fault (Limit): stack overflow"), "{out}");
}

const FIELDS: &str = r#"type Sq = { side: Int, name: String }
type Ci = { name: String, r: Int }

fn label(s: Sq | Ci) -> String { s.name }

fn main() {
  print(label(Ci { name: "c", r: 2 }))
  print(label(Sq { side: 1, name: "q" }))
  mut s: Sq | Ci = Ci { name: "c", r: 2 }
  s.name = "renamed"
  print(s)
  mut t: Sq | Ci = Sq { side: 3, name: "s" }
  t.name = t.name + "!"
  print(t)
  mut pair := (1, "a")
  pair.1 = "b"
  pair.0 = pair.0 + 1
  print(pair)
  mut nested := ((1, 2), "x")
  nested.0.1 = 9
  print(nested)
}
"#;

const TRAIT_EQUALITY: &str = r#"trait Area {
  fn area(self) -> Int
}
type Sq = { side: Int }
type Ci = { r: Int }
impl Area for Sq { fn area(self) -> Int { self.side * self.side } }
impl Area for Ci { fn area(self) -> Int { 3 * self.r * self.r } }
impl Area for Int { fn area(self) -> Int { self } }

fn main() {
  let a: Area = Sq { side: 2 }
  let b: Area = Sq { side: 2 }
  let c: Area = Ci { r: 2 }
  let d: Area = 4
  print(a == b)
  print(d == d)
  let xs: List[Area] = [a, c, d]
  print(contains(xs, b))
  let ys: List[Area] = [Sq { side: 2 }, Ci { r: 2 }, 4]
  print(xs == ys)
  for (i, x) in enumerate(xs) { print(f"{i}: {x.area()}") }
  for 0 in [0, 0] { print("zero") }
}
"#;

const STACK: &str = r#"trait Describe {
  fn describe(self) -> String
}
type Thing = { n: Int }
impl Describe for Thing {
  fn describe(self) -> String { f"{inner(self.n)}" }
}
impl Thing {
  fn twice(self) -> Int { self.n * 2 }
}
fn inner(k: Int) -> Int { [1, 2].map(x -> half(x + k)).fold(0, (a, b) -> a + b) }
fn half(n: Int) -> Int requires n % 2 == 0 { n / 2 }
fn main() {
  print(Thing { n: 2 }.twice())
  print(Thing { n: 1 }.describe())
}
"#;

/// Unions with members that aren't records or variants (`Int | String`):
/// boxed members, narrowing in arms, type and literal patterns, `==`,
/// printing, trait methods on a union, refinement checks of a union.
#[test]
fn unions_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("unions");
    let (out, code) = case(&d, "unions", UNIONS);
    assert!(code == 0 && out.starts_with("number 4\ntext 4\nzero\npositive 5\nnegative\n") && out.contains("[1, \"a\", 2]"), "{out}");
    let (out, code) = case(&d, "union_methods", UNION_METHODS);
    assert!(code == 101 && out.contains("-1 is not a `Positive | String`"), "{out}");
}

#[test]
fn declared_unions_and_parametric_type_patterns() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("declared_unions");
    let (out, code) = case(&d, "declared", DECLARED_UNIONS);
    assert_eq!((out.as_str(), code), ("text\n3\nlist 2\ntext\nempty\n", 0));
}

// a variable declared a union stays one (it was narrowed to its first
// value's type); `List[Int]` is a type pattern
const DECLARED_UNIONS: &str = r#"fn describe(v: List[Int] | String) -> String {
  match v {
    List[Int] => f"list {len(v)}",
    String => "text",
  }
}

fn main() {
  mut u: Int | String = 3
  u = "text"
  print(u)
  let w: Int | String = 3
  print(w)
  print(describe([1, 2]))
  print(describe("x"))
  let e: List[Int] | String = []
  match e {
    List[Int] => print("empty"),
    String => print("text"),
  }
}
"#;

const UNIONS: &str = r#"type Sq = { side: Int }

fn describe(v: Int | String) -> String {
  match v {
    Int => f"number {v}",
    String => f"text {len(v)}",
  }
}

fn kind(v: Int | String | Sq | List[Int]) -> String {
  match v {
    0 => "zero",
    Positive => f"positive {v}",
    Int => "negative",
    "" => "empty text",
    String => f"text {v}",
    Sq => f"square {v.side}",
    [] => "empty list",
    [x, ..rest] => f"list from {x}, {len(rest)} more",
  }
}

fn pick(b: Bool) -> Int | String {
  if b { 1 } else { "one" }
}

fn main() {
  print(describe(4))
  print(describe("four"))
  for x in [0, 5, -2] { print(kind(x)) }
  print(kind(""))
  print(kind("hey"))
  print(kind(Sq { side: 3 }))
  print(kind([]))
  print(kind([1, 2]))
  let xs: List[Int | String] = [1, "a", 2]
  print(xs)
  print(pick(true) == pick(true))
  print(pick(true) == pick(false))
  print(to_string(pick(false)))
  print(xs.map(x -> describe(x)))
  let u: Int | String = pick(false)
  print(u)
  print(u is String)
  print(u is Int)
}
"#;

const UNION_METHODS: &str = r#"trait Area {
  fn area(self) -> Int
}
type Sq = { side: Int }
type Ci = { r: Int }
impl Area for Sq { fn area(self) -> Int { self.side * self.side } }
impl Area for Ci { fn area(self) -> Int { 3 * self.r * self.r } }
impl Area for Int { fn area(self) -> Int { self } }

fn total(xs: List[Sq | Ci | Int]) -> Int { xs.map(x -> x.area()).fold(0, (a, b) -> a + b) }

fn f(x: Positive | String) -> String { f"{x}" }

fn main() {
  print(total([Sq { side: 2 }, Ci { r: 1 }, 5]))
  let s: Sq | Ci = Ci { r: 2 }
  print(s.area())
  print(f(3))
  print(f("a"))
  let n: Int = 0 - 1
  print(f(n))
}
"#;

/// Methods called on trait values that mention `Self`, take a `mut`
/// parameter or type parameters of their own; a generic supertrait's
/// methods; a function returning a trait-typed value.
#[test]
fn trait_value_methods_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("trait-methods");
    let (out, code) = case(&d, "trait_methods", TRAIT_METHODS);
    assert!(code == 0 && out.contains("(\"sq2\", \"x\")") && out.ends_with("pretty square\n<4>\n100\ncircle\n"), "{out}");
}

const TRAIT_METHODS: &str = r#"trait Named {
  fn name(self) -> String
}

trait Shape: Named {
  fn area(self) -> Int
  fn bigger(self, other: Self) -> Self
  fn scaled(self, k: Int) -> Self
  fn tag[U](self, extra: U) -> (String, U)
  fn grow(mut self, by: Int)
  fn count(self, mut seen: Int) -> Int
}

type Sq = { side: Int }
type Ci = { r: Int }

impl Named for Sq { fn name(self) -> String { "square" } }
impl Named for Ci { fn name(self) -> String { "circle" } }

impl Shape for Sq {
  fn area(self) -> Int { self.side * self.side }
  fn bigger(self, other: Self) -> Self { if other.side > self.side { other } else { self } }
  fn scaled(self, k: Int) -> Self { Sq { side: self.side * k } }
  fn tag[U](self, extra: U) -> (String, U) { (f"sq{self.side}", extra) }
  fn grow(mut self, by: Int) { self.side += by }
  fn count(self, mut seen: Int) -> Int { seen += 1 seen }
}

impl Shape for Ci {
  fn area(self) -> Int { 3 * self.r * self.r }
  fn bigger(self, other: Self) -> Self { if other.r > self.r { other } else { self } }
  fn scaled(self, k: Int) -> Self { Ci { r: self.r * k } }
  fn tag[U](self, extra: U) -> (String, U) { (f"ci{self.r}", extra) }
  fn grow(mut self, by: Int) { self.r += by }
  fn count(self, mut seen: Int) -> Int { seen += 10 seen }
}

trait Convert[T] {
  fn convert(self) -> T
}
trait Pretty: Convert[String] {
  fn pretty(self) -> String
}
impl Convert[String] for Sq { fn convert(self) -> String { f"<{self.side}>" } }
impl Pretty for Sq { fn pretty(self) -> String { "pretty square" } }

fn make(big: Bool) -> Shape {
  if big { Sq { side: 10 } } else { Ci { r: 1 } }
}

fn main() {
  let shapes: List[Shape] = [Sq { side: 2 }, Ci { r: 1 }]
  for s in shapes {
    print(s.name())
    print(s.area())
    print(s.scaled(3).area())
    print(s.bigger(s.scaled(2)).area())
    print(s.tag(7))
    print(s.tag("x"))
    mut n := 0
    print(s.count(mut n))
    print(n)
  }
  mut g: Shape = Ci { r: 2 }
  mut g.grow(3)
  print(g.area())
  let p: Pretty = Sq { side: 4 }
  print(p.pretty())
  print(p.convert())
  print(make(true).area())
  print(make(false).name())
}
"#;

/// `matches` with a literal pattern, compiled to a DFA (the interpreter's
/// engine): ASCII and Unicode classes, in a refinement too; a bad pattern
/// faults with the interpreter's message. And functions shown as values.
#[test]
fn regexes_and_shown_functions_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("regex");
    let (out, code) = case(&d, "regex", REGEX);
    assert!(code == 101 && out.starts_with("abc: true\nabc_d1: true\nAbc: false\n") && out.contains("does not satisfy `Sku`"), "{out}");
    let (out, code) = case(&d, "bad_regex", "fn main() { print(matches(\"a\", \"(\")) }\n");
    assert!(code == 101 && out.contains("bad regex: regex parse error"), "{out}");
    let (out, code) = case(&d, "shown_functions", SHOWN_FUNCTIONS);
    assert!(code == 0 && out.starts_with("<closure (x) -> …>\nH { name: \"h\", on: <fn twice> }\n"), "{out}");
}

const REGEX: &str = r#"type Sku = String where matches(it, "[A-Z]{3}-[0-9]{4}")

fn check(s: String) -> Bool { matches(s, "[a-z]+(_[a-z0-9]+)*") }

fn main() {
  for s in ["abc", "abc_d1", "Abc", "", "a__b", "héllo"] {
    print(f"{s}: {check(s)}")
  }
  print(matches("héllo", "\\w+"))
  print(matches("日本語", "\\p{Han}+"))
  print(matches("x", "(a|b|x)"))
  print(matches("2024-01-31", "\\d{4}-\\d{2}-\\d{2}"))
  print("ab".matches("a.?b"))
  let k: Sku = "ABC-1234"
  print(k)
  let s: String = "abc-1234"
  let bad: Sku = s
  print(bad)
}
"#;

const SHOWN_FUNCTIONS: &str = r#"fn twice(n: Int) -> Int { n * 2 }
type H = { name: String, on: (Int) -> Int }
fn main() {
  k := 3
  f := (x: Int) -> x + k
  print(f)
  print(H { name: "h", on: twice })
  print([f, (a: Int) -> a])
  print(f(1))
}
"#;

/// `Int` never overflows, compiled too: small integers are themselves, any
/// other is a big integer (runtime.c, "integers"), through arithmetic,
/// Euclidean `/` and `%`, comparisons, equality, ordering, printing,
/// `float`, `floor`, `parse_int`, literals and patterns.
#[test]
fn big_integers_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("bigint");
    let (out, code) = case(&d, "bigint", BIG_INTEGERS);
    assert!(code == 0 && out.starts_with("15511210043330985984000000\n870\n354224848179261915075\n") && out.contains("9223372036854775808\n-9223372036854775809\n"), "{out}");
}

const BIG_INTEGERS: &str = r#"fn factorial(n: Int) -> Int { if n <= 1 { 1 } else { n * factorial(n - 1) } }

fn fib(n: Int) -> Int {
  mut a := 0
  mut b := 1
  for _ in 0..n {
    t := a + b
    a = b
    b = t
  }
  a
}

fn main() {
  print(factorial(25))
  print(factorial(30) / factorial(28))
  print(fib(100))
  print(fib(100) % 1000007)
  big := 123456789012345678901234567890
  print(big)
  print(-big)
  print(big * big)
  print(big / -7)
  print(big % -7)
  print(-big / 7)
  print(-big % 7)
  print(big == 123456789012345678901234567890)
  print(big > fib(100))
  print(big < -big)
  print(abs(-big))
  print(min(big, 3))
  print(max(-big, -3))
  print(float(big))
  print(floor(1000000000000000000000000000000.0))
  print(parse_int("99999999999999999999999"))
  print(parse_int("-4611686018427387905"))
  n := 4611686018427387903
  print(n + 1)
  print(n + 1 - 1 == n)
  print([big, 1, -big].sort())
  print(Set { big, big * 1, 2 })
  print(Dict { big: "big" })
  print(f"{big:>40}")
  print(9223372036854775807 + 1)
  print(-9223372036854775808 - 1)
  print(abs(-4611686018427387904))
  match n + 1 {
    4611686018427387904 => print("matched big"),
    _ => print("no"),
  }
  print(2 * 3 + 4 - 5)
  print(len([1, 2, 3]) * 1000000000000000000000)
  // big integers are freed: this loop's thousands of them leave none live
  mut acc: Int = big
  mut i: Int = 0
  while i < 2000 {
    acc = (acc * big + i) % (big * big)
    i = i + 1
  }
  print(acc > 0)
  ys := [big * 2, big * 3]
  zs := ys.push(big * 4)
  print(len(zs))
}
"#;

/// `mock T` compiled: methods whose results are generated within the
/// result's refinements (bounds, lengths, regexes; rejection otherwise)
/// until `ensures` holds, after `requires` (whose fault is the
/// interpreter's). The values are not the interpreter's; what they satisfy is.
#[test]
fn mocks_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("mock");
    let (out, code) = case(&d, "mock", MOCKS);
    assert!(code == 101 && out.starts_with("<mock Store>\nall good\nfault (Requires): precondition of `Store.find` violated: requires id > 0"), "{out}");
    let (out, code) = case(&d, "orders", &tests_as_main("examples/orders.strata", ""));
    assert!(code == 0 && out.ends_with("all asserts hold\n"), "{out}");
}

const MOCKS: &str = r#"type Item = { name: NonEmptyString, qty: Positive }
type Color = | Red | Green | Rgb(Int, Int, Int)

trait Store {
  fn count(self) -> Natural
  fn find(self, id: Int) -> Option[Item]
    requires id > 0
  fn all(self) -> List[Item]
    ensures len(it) <= 3
  fn color(self) -> Color
  fn code(self) -> String where matches(it, "[A-Z]{2}[0-9]{3}")
  fn save(mut self, i: Item) -> Result[(), String]
  fn rate(self) -> Float where 0.0 <= it && it <= 1.0
}

fn main() {
  mut s := mock Store
  for _ in 0..20 {
    n := s.count()
    assert n >= 0
    match s.find(3) {
      Some(i) => { assert len(i.name) > 0 && i.qty > 0 },
      None => (),
    }
    assert len(s.all()) <= 3
    c := s.code()
    assert matches(c, "[A-Z]{2}[0-9]{3}")
    r := s.rate()
    assert 0.0 <= r && r <= 1.0
    k := s.color()
    assert k is Red || k is Green || k is Rgb
    res := mut s.save(Item { name: "a", qty: 1 })
    assert res is Ok || res is Err
  }
  print(s)
  print("all good")
  print(s.find(0))
}
"#;

/// `strata test` compiles the tests with the JIT: the same report as
/// interpreted, for every example and for failing tests (fault, call stack,
/// what they printed).
#[test]
fn the_jit_runs_tests_as_the_interpreter() {
    let strata = env!("CARGO_BIN_EXE_strata");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let report = |file: &Path, how: &str| output(Command::new(strata).arg("test").arg(how).arg("--file").arg(file));
    for ex in ["bank.strata", "builtins.strata", "math.strata", "orders.strata", "traits.strata", "tasks", "todos", "wc", "page"] {
        let f = root.join("examples").join(ex);
        let jit = report(&f, "--jit");
        assert!(!jit.0.contains("note: interpreted"), "{ex}: {}", jit.0);
        assert_eq!(jit, report(&f, "--interpret"), "the JIT (left) and the interpreter (right) differ for {ex}");
    }
    let d = tmp("jit-tests");
    let f = d.join("t.strata");
    save(&f, "fn half(n: Int) -> Int requires n % 2 == 0 { n / 2 }\nfn twice(n: Int) -> Int { half(n) * 4 }\ntest \"passes\" { assert twice(4) == 8 }\ntest \"fails\" { print(\"before\") print(twice(3)) }\ntest \"asserts\" { x := [1, 2] assert len(x) == 3 }\n");
    let jit = report(&f, "--jit");
    assert_eq!(jit, report(&f, "--interpret"));
    assert!(jit.0.contains("✗ fails\n    Requires: precondition of `half` violated") && jit.0.contains("    stack: twice -> half\n    printed:\n      before\n") && jit.0.ends_with("1 passed, 2 failed\n"), "{}", jit.0);
}

/// `strata verify` fuzzes compiled (by default, as `--jit`): the same
/// verdicts as the interpreter's fuzzer (its inputs are the runtime's), a
/// runaway loop stopped by the step budget.
#[test]
fn verify_fuzzes_compiled() {
    let strata = env!("CARGO_BIN_EXE_strata");
    let d = tmp("jit-verify");
    let f = d.join("v.strata");
    save(&f, "fn pieces(s: String) -> Positive { len(split(s, \"a\")) - 1 }\nfn loops(n: Natural) -> Int { mut i := 0 while i != n * 2 + 1 { i += 2 } i }\nfn fine(xs: List[String]) -> Natural { len(join(xs, \",\")) }\n");
    let (out, _) = output(Command::new(strata).args(["verify", "--jit", "--file"]).arg(&f).env("STRATA_SOLVER", "none"));
    assert!(!out.contains("note:"), "{out}");
    assert!(out.contains("result of `pieces`: 0 does not satisfy `Positive`") && out.contains("~ fine                     fuzzed: 200 cases, 0 discarded"), "{out}");
    let (out, _) = output(Command::new(strata).args(["verify", "--file"]).arg(&f).env("STRATA_SOLVER", "none"));
    assert!(!out.contains("note:"), "{out}");
    assert!(out.contains("✗ pieces                   Refinement violation (found by the fuzzer)") && out.contains("result of `pieces`: 0 does not satisfy `Positive`"), "{out}");
    assert!(out.contains("✗ loops                    Limit violation (found by the fuzzer)") && out.contains("too many steps (stack: loops)"), "{out}");
    assert!(out.contains("~ fine                     fuzzed: 200 cases, 0 discarded"), "{out}");
}

// No limit on recursion or loops: a recursion runs until the stack is
// nearly out, then faults (as compiled code does), its calls one entry of
// the stack line; a test's runaway recursion fails that test.
#[test]
fn recursion_runs_until_the_stack_is_out() {
    let d = tmp("no_limits");
    let strata = env!("CARGO_BIN_EXE_strata");
    let f = d.join("deep.strata");
    save(
        &f,
        r#"type Chain =
  | Link(Int, Chain)
  | End

fn down(n: Int) -> Int {
  if n == 0 { 0 } else { 1 + down(n - 1) }
}

fn length(c: Chain) -> Int {
  match c {
    Link(_, rest) => 1 + length(rest),
    End => 0,
  }
}

fn main() {
  print(down(20000))
  mut c: Chain = End
  mut i: Int = 0
  while i < 1500000 {
    i = i + 1
  }
  i = 0
  while i < 20000 {
    c = Link(i, c)
    i = i + 1
  }
  print(length(c))
  print(down(1000000000))
}

test "runaway" {
  assert down(100) == 100
  down(-1)
}
"#,
    );
    let o = Command::new(strata).args(["run", "--interpret"]).arg(&f).output().unwrap();
    let (out, err) = (String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert_eq!((out.as_ref(), o.status.code()), ("20000\n20000\n", Some(101)), "{err}");
    assert!(err.starts_with("fault (Limit): stack overflow (recursion too deep)\n    stack: main -> down ("), "{err}");
    assert!(err.ends_with(" times)\n") && err.lines().count() == 2, "{err}");
    for how in ["--interpret", "--jit"] {
        let o = Command::new(strata).args(["test", how, "--file"]).arg(&f).output().unwrap();
        let out = String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr);
        assert!(out.contains("✗ runaway\n    Limit: stack overflow (recursion too deep)\n    stack: down (") && out.contains("0 passed, 1 failed"), "{how}: {out}");
    }

}

#[path = "common/json.rs"]
mod json_case;

/// `derive Json` and `std.json`'s text compiled: what the interpreter prints.
#[test]
fn json_compiles() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("derive-json");
    let (out, code) = case(&d, "json", json_case::PROGRAM);
    assert_eq!((out.as_str(), code), (json_case::OUTPUT, 0));
}

// `extern c`: the package's C (`[native]`), called from compiled code with
// each type converted both ways; the same output built and with the JIT.
#[test]
fn extern_c_calls() {
    if !have_clang() {
        return;
    }
    let d = tmp("extern_c");
    let strata = env!("CARGO_BIN_EXE_strata");
    std::fs::write(d.join("strata.toml"), "name = \"ffi\"\n\n[native]\nsources = [\"ffi.c\"]\n").unwrap();
    std::fs::write(
        d.join("ffi.c"),
        r#"#include <ctype.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
int8_t neg8(int8_t x) { return (int8_t)-x; }
uint8_t inc8(uint8_t x) { return (uint8_t)(x + 1); }
uint64_t umax(void) { return UINT64_MAX; }
int64_t imin(void) { return INT64_MIN; }
uint64_t half(uint64_t x) { return x / 2; }
bool even(int32_t x) { return x % 2 == 0; }
double scale(double x, bool twice) { return twice ? x * 2 : x; }
const char *shout(const char *s) {
  static char buf[64];
  size_t i = 0;
  for (; s[i] && i < 63; i++) buf[i] = (char)toupper((unsigned char)s[i]);
  buf[i] = 0;
  return buf;
}
typedef struct { int32_t n; } Counter;
int32_t counter_open(int32_t start, Counter **out) {
  if (start < 0) { *out = NULL; return 1; }
  *out = malloc(sizeof **out);
  (*out)->n = start;
  return 0;
}
int32_t counter_bump(Counter *c) { return ++c->n; }
void counter_close(Counter *c) { free(c); }
Counter *nothing(void) { return NULL; }
void twice(int32_t *x) { *x *= 2; }
"#,
    )
    .unwrap();
    let (ok, out) = (|| {
        let o = Command::new(strata).args(["--file", ".", "edit", "c"]).current_dir(&d).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let mut o = o;
        o.stdin.take().unwrap().write_all(
            br#"extern c type Counter

extern c fn neg8(x: I8) -> I8
extern c fn inc8(x: U8) -> U8
extern c fn umax() -> U64
extern c fn imin() -> I64
extern c fn half(x: U64) -> U64
extern c fn even(x: I32) -> Bool
extern c fn scale(x: Float, twice: Bool) -> Float
extern c fn shout(s: String) -> String
extern c fn counter_open(start: I32, mut c: Option[Counter]) -> I32
extern c fn counter_bump(c: Counter) -> I32
extern c fn counter_close(c: Counter)
extern c fn nothing() -> Counter
extern c fn twice(mut x: I32)

pub fn numbers() -> List[Int] {
  [neg8(5), neg8(-128), inc8(254), umax(), imin(), half(umax()), (if even(4) { 1 } else { 0 })]
}

pub fn texts() -> String {
  f"{shout("hello, c")} {scale(1.5, true)} {scale(1.5, false)}"
}

pub fn counting() -> List[Int] {
  mut c: Option[Counter] = None
  code := counter_open(41, mut c)
  mut x: I32 = 21
  twice(mut x)
  match c {
    Some(h) => {
      first := counter_bump(h)
      second := counter_bump(h)
      counter_close(h)
      [code, first, second, x]
    }
    None => [code],
  }
}

pub fn refused() -> List[Int] {
  mut c: Option[Counter] = None
  code := counter_open(-1, mut c)
  [code, (if c == None { 1 } else { 0 })]
}

pub fn null_handle() -> Int {
  counter_bump(nothing())
}

pub fn nul_string() -> String {
  shout("a" + chr(0) + "b")
}
"#,
        )
        .unwrap();
        let r = o.wait_with_output().unwrap();
        (r.status.success(), String::from_utf8_lossy(&r.stderr).to_string())
    })();
    assert!(ok, "{out}");
    save(
        &d.join("main.strata"),
        "use c\n\nfn main() {\n  print(c.numbers())\n  print(c.texts())\n  print(c.counting())\n  print(c.refused())\n}\n\ntest \"null\" {\n  c.null_handle()\n}\n\ntest \"nul\" {\n  c.nul_string()\n}\n",
    );
    // (beyond 64 bits, an `Int` shows as a string of digits)
    let want = "[-5, -128, 255, 18446744073709551615, -9223372036854775808, 9223372036854775807, 1]\nHELLO, C 3.0 1.5\n[0, 42, 43, 42]\n[1, 1]\n";
    // built: the same output, and every object freed
    let exe = d.join("prog");
    let b = Command::new(strata).args(["build", "-o"]).arg(&exe).current_dir(&d).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    let (out, code) = freed_everything(output(Command::new(&exe).current_dir(&d).env("STRATA_MEMORY", "check")), &d);
    assert_eq!((out.as_str(), code), (want, 0));
    {
        let (out, code) = output(Command::new(strata).args(["run", "--jit", "."]).current_dir(&d));
        assert_eq!((out.as_str(), code), (want, 0));
        // compiled tests: C's NULL for a handle, a NUL in a string
        let (out, _) = output(Command::new(strata).args(["test", "--jit"]).current_dir(&d));
        assert!(out.contains("`nothing` gave NULL for a `Counter` (declare it `Option[Counter]` if it can be NULL)"), "{out}");
        assert!(out.contains("a string passed to `shout` holds a NUL character"), "{out}");
    }
    // the interpreter doesn't call C
    let (out, code) = output(Command::new(strata).args(["run", "--interpret", "."]).current_dir(&d));
    assert!(code != 0 && out.contains("`neg8` is an `extern c` function: only compiled code calls it"), "{out}");
}

/// Types passed whole: compiled as interpreted.
#[test]
fn impl_type_parameters_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("impl-statics");
    for (name, prog, want) in [("statics", json_case::STATICS, json_case::STATICS_OUTPUT), ("builtins", json_case::BUILTINS, json_case::BUILTINS_OUTPUT), ("text", json_case::TEXT, json_case::TEXT_OUTPUT)] {
        let (out, code) = case(&d, name, prog);
        assert_eq!((out.as_str(), code), (want, 0));
    }
}
