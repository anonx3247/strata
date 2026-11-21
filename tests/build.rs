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
    let native = output(Command::new(&exe).args(ARGS).current_dir(dir));
    let interp = output(Command::new(strata).arg("run").arg(file).arg("--").args(ARGS).current_dir(dir));
    assert_eq!(native, interp, "native (left) and interpreted (right) runs differ for {}", file.display());
    native
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
        ("assert", "fn main() -> () { let s = \"a\\\"b\" assert s + \"!\" == \"x\" }", 101),
        ("assignment", "fn main() -> () { let mut x: Int where it < 5 = 0 while true { x = x + 1 } }", 101),
        ("division", "fn main() -> () { print(-7 / 2) print(-7 % 2) print(7 / -2) print(1 / 0) }", 101),
        ("floats", "fn main() -> () { print(0.1 + 0.2) print(1000000000000000000000.0) print(0.00000015) print(100.0) print(-5.5 % 2.0) print(1.0 / 0.0) }", 101),
        ("exit_code", "fn main() -> Int { 3 }", 3),
        (
            "loops",
            "fn g(n: Int) -> Int { let mut i = 0 while i < 10 { i = i + 1 if i == n { break } if i % 2 == 0 { continue } } i }\n\
             fn pick(b: Bool) -> String { match b { true => \"yes\", false => return \"no\" } }\n\
             fn main() -> () { print(g(4)) print(g(40)) print(pick(true) + pick(false)) print(f\"{1.5} {true} {\"q\"} {len(\"héllo\")}\") }",
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
    assert!(out.contains("{\"Err\":\"3 is odd\"}") && out.contains("big circle, circle, rect or dot, nothing"), "{out}");
    assert_eq!(code, 101);
    let cases = [
        ("field_refinement", "type Quantity = Int where 1 <= it && it <= 99\ntype Line = { sku: String, qty: Quantity }\nfn main() -> () { let mut l = Line { sku: \"a\", qty: 5 } l.qty = 0 }", 101),
        (
            "tree",
            "type Tree[T] =\n  | Leaf\n  | Node(Tree[T], T, Tree[T])\n\
             fn insert(t: Tree[Int], x: Int) -> Tree[Int] { match t { Leaf => Node(Leaf, x, Leaf), Node(l, v, r) => if x < v { Node(insert(l, x), v, r) } else { Node(l, v, insert(r, x)) } } }\n\
             fn size[T](t: Tree[T]) -> Int { match t { Leaf => 0, Node(l, _, r) => size(l) + 1 + size(r) } }\n\
             fn main() -> () { let mut t: Tree[Int] = Leaf for i in range(0, 10) { t = insert(t, (i * 7) % 10) } print(size(t)) print(insert(insert(Leaf, 2), 1)) }",
            0,
        ),
        ("own_methods", "type Item = { name: String, price: Int }\nimpl Item {\n  fn new(name: String) -> Item { Item { name: name, price: 0 } }\n  fn label(self) -> String { f\"{self.name}: {self.price}\" }\n}\nfn main() -> () { print(Item.new(\"cup\").label()) }", 0),
        ("refined_argument", "fn f(o: Option[Positive]) -> Int { o ?? 0 }\nfn main() -> () { print(f(Some(3))) let x: Option[Int] = Some(-2) print(f(x)) }", 101),
        ("main_err", "fn main() -> Result[(), String] { print(\"working\") Err(\"it broke\") }", 1),
        (
            "errors",
            "type E =\n  | NotFound\n  | Bad { why: String }\n\
             fn g(n: Int) -> Result[Int, E] { if n == 0 { Err(NotFound) } else if n < 0 { Err(Bad { why: \"neg\" }) } else { Ok(n) } }\n\
             fn h(n: Int) -> Result[Int, E] { let v = g(n)! Ok(v * 2) }\n\
             fn main() -> () { print(h(3)) print(h(0)) print(h(-1)) print(h(0) == Err(NotFound)) let (x, y) = (5, \"s\") print(f\"{x}{y}\") }",
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
    assert!(out.contains("[2,3,5,7,11,13,17,19,23,29]") && out.contains("[0]: -1 does not satisfy `Positive`"), "{out}");
    assert_eq!(code, 101);
    // examples/math.strata, with a `main` that prints what its test checks
    let math = Command::new(env!("CARGO_BIN_EXE_strata")).args(["render", "--file"]).arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/math.strata")).output().unwrap();
    let math = String::from_utf8(math.stdout).unwrap();
    let math = math[..math.find("test \"values\"").unwrap()].to_string()
        + "fn main() -> () {\n  print([factorial(5), gcd(12, 18), power(2, 10), digit_sum(1234), ackermann(2, 3), triangle(10), isqrt(17), collatz_steps(6)])\n  print(positive_squares([-2, 3, 0, 4]))\n  print(primes(20))\n  print(first_or_one([5, 1]))\n  print(average([]))\n}\n";
    let (out, code) = case(&d, "math", &math);
    assert!(out.contains("[120,6,1024,10,9,55,4,8]") && out.contains("division by zero"), "{out}");
    assert_eq!(code, 101);
    let (out, code) = case(&d, "range", "fn main() -> () { for i in range(0, 2000000) { print(i) } }");
    assert!(out.contains("range too large") && code == 101, "{out}");
}

#[test]
fn traits_run_as_interpreted() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("traits");
    // examples/traits.strata, its tests as `main` (without the regex of `Sku`)
    let src = tests_as_main("examples/traits.strata", "Sku(");
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
        ("mut_params", "fn restock(mut n: Int, extra: Int) -> () { n = n + extra }\ntype Line = { sku: String, qty: Int }\nfn bump(mut l: Line) -> () { l.qty = l.qty + 1 }\nfn main() -> () { let mut k = 1 restock(mut k, 5) print(k) let mut line = Line { sku: \"a\", qty: 1 } bump(mut line) bump(mut line) print(line) restock(mut line.qty, 10) print(line) }"),
        ("files_and_environment", "use std.io.Console\nuse std.fs.{Files, FsError}\nuse std.process.Environment\nfn main(mut console: Console, mut files: Files, env: Environment) -> Result[(), String] {\n  let _ = mut files.remove(\"fsplay/b.txt\")\n  let _ = mut files.remove(\"fsplay/a.txt\")\n  let _ = mut files.remove(\"fsplay/sub/deep\")\n  let _ = mut files.remove(\"fsplay/sub\")\n  let _ = mut files.remove(\"fsplay\")\n  print(files.exists(\"fsplay\"))\n  print(mut files.make_dir(\"fsplay/sub/deep\"))\n  print(mut files.write(\"fsplay/a.txt\", \"one\\n\"))\n  print(mut files.append(\"fsplay/a.txt\", \"two\\n\"))\n  print(files.read(\"fsplay/a.txt\"))\n  print(files.read(\"fsplay/missing.txt\"))\n  print(files.read(\"fsplay/sub\"))\n  print(mut files.write(\"fsplay/b.txt\", \"x\"))\n  print(files.list(\"fsplay\"))\n  print(files.list(\"fsplay/a.txt\"))\n  print(mut files.remove(\"fsplay/nope\"))\n  print(env.args())\n  print(env.var(\"HOME\") is Some)\n  print(env.var(\"STRATA_SURELY_UNSET_VAR\"))\n  mut console.print(\"done\")\n  Ok(())\n}"),
        ("one_function_two_consoles", "use std.io.{Console, recorder}\nfn report(mut console: Console, n: Int) -> () {\n  for i in range(0, n) { mut console.print(f\"line {i}\") }\n}\nfn main(mut console: Console) -> () {\n  let mut r = recorder([])\n  report(mut r, 3)\n  report(mut console, 2)\n  print(r.out)\n  print(r)\n}"),
    ];
    for (name, src) in cases {
        let (out, code) = case(&d, name, src);
        assert_eq!(code, 0, "{name}: {out}");
    }
    // the files test made and read a folder of its own, there
    assert!(d.join("fsplay/a.txt").exists());
    let (out, _) = case(&d, "args", "use std.process.Environment\nfn main(env: Environment) -> () { print(env.args()) }");
    assert!(out.contains(r#"["one","two words"]"#), "{out}");
}

/// A program's text (rendered), its tests turned into blocks of a `main`.
fn tests_as_main(file: &str, skip: &str) -> String {
    let src = Command::new(env!("CARGO_BIN_EXE_strata")).args(["render", "--file"]).arg(Path::new(env!("CARGO_MANIFEST_DIR")).join(file)).output().unwrap();
    let src = String::from_utf8(src.stdout).unwrap();
    let mut decls = String::new();
    let mut main = String::from("fn main() -> () {\n");
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
    assert!(out.contains(r#"["2\t3\t14\ta.txt","1\t2\t12\tb.txt","3\t5\t26\ttotal"]"#) && out.contains("NotFound") && code == 0, "{out}");
    let (out, code) = case(&d, "strings", STRINGS);
    assert!(out.contains(r#"["STRASSE Ǆ ʼN","οδος σ ας. i̇","SS","ǆ"]"#) && out.contains("[-00042]") && code == 0, "{out}");
    let (out, code) = case(&d, "dicts_and_sets", COLLECTIONS);
    assert!(out.contains(r#"{"cat":2,"end":1,"hat":1,"the":3}"#) && out.contains("[[1,\"a\"],[2,\"b\"],[3,\"z\"]]") && code == 0, "{out}");
    let faults = [
        ("dict_value", "fn f() -> Dict[String, Positive] { Dict { \"a\": 1, \"b\": 0 } }\nfn main() -> () { print(f()) }"),
        ("set_element", "fn g(s: Set[Positive]) -> Int { len(s) }\nfn main() -> () { print(g(Set { 1 })) print(g(Set { 3, -2 })) }"),
        ("dict_key", "fn main() -> () { let d: Dict[NonEmptyString, Int] = Dict { \"x\": 1, \"\": 2 } print(d) }"),
        ("set_assigned", "fn main() -> () { let mut s: Set[Natural] = Set { 1 } s = insert(s, -1) }"),
        ("split", "fn main() -> () { print(split(\"abc\", \"\")) }"),
        ("replace", "fn main() -> () { print(replace(\"abc\", \"\", \"x\")) }"),
        ("repeat", "fn main() -> () { print(repeat(\"ab\", 3)) print(repeat(\"ab\", 60000000)) }"),
        ("ord", "fn main() -> () { print(ord(\"a\")) print(ord(\"a\\\"\\n\")) }"),
        ("chr", "fn main() -> () { print(chr(55296)) }"),
        ("string_index", "fn main() -> () { print(\"abc\"[2]) print(\"abc\"[5]) }"),
        ("list_index", "fn main() -> () { print([1, 2][-1]) }"),
    ];
    for (name, src) in faults {
        let (out, code) = case(&d, name, src);
        assert!(code == 101 && out.contains("fault ("), "{name}: {out}");
    }
}

const WC_MAIN: &str = r#"fn main() -> () {
  let files = mem_files([("a.txt", "one two\nthree\n"), ("b.txt", "four  five" + chr(13) + "\n")])
  for (only, names) in [(None, ["a.txt", "b.txt"]), (Some(Words), ["a.txt"]), (Some(Chars), ["b.txt", "a.txt"]), (None, ["nope.txt"])] {
    let mut console = recorder([])
    let r = wc(Options { only: only, files: names }, mut console, files)
    print(r)
    print(printed(console))
  }
  let mut console = recorder(["hello world", "bye"])
  print(wc(Options { only: None, files: [] }, mut console, mem_files([])))
  print(printed(console))
  print(count("a b  c\n\nd"))
}
"#;

const STRINGS: &str = r#"fn main() -> () {
  let cr = chr(13)
  let text = "one two" + cr + "\nthree\n\nfour" + cr
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
  let x = 3.14159
  let n = -42
  let s = "héllo"
  print(f"[{x:.2}] [{x:8.3}] [{n:>6}] [{n:<6}] [{n:^7}] [{n:06}] [{n:+}] [{7:+05}] [{s:.2}] [{s:>8}] [{s:*^9}] [{s:3}] [{0.125:.2}] [{true:>6}] [{-0.0:.1}]")
  let w = "abc"
  print([w[0], w[2], w[1..], w[..2], w[..]])
  let xs = [10, 20, 30, 40]
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
  let mut d: Dict[String, Int] = Dict {}
  for w in words {
    d = insert(d, w, (get(d, w) ?? 0) + 1)
  }
  d
}

fn main() -> () {
  let d = Dict { "b": 2, "a": 1, "c": 3 }
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
  let s = Set { 3, 1, 3, 2 }
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
  let nested = Dict { "k": Dict { 2: "two", 1: "one" } }
  print(nested)
  print(f"{d} {s}")
  print(to_string(s) + to_json(d))
  print([d][0])
}
"#;

#[test]
fn what_is_not_compiled_yet_is_named() {
    let d = tmp("unsupported");
    let f = d.join("lists.strata");
    save(&f, "fn main() -> () { print(matches(\"a1\", \"[a-z][0-9]\")) }\n");
    let o = Command::new(env!("CARGO_BIN_EXE_strata")).arg("build").arg(&f).arg("--emit-llvm").output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success() && err.contains("can't compile `main`: the built-in `matches` on (String, String) is not compiled yet"), "{err}");
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
#[cfg(feature = "jit")]
#[test]
fn the_jit_runs_as_the_interpreter() {
    let d = tmp("jit");
    let strata = env!("CARGO_BIN_EXE_strata");
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/build/numbers.strata");
    let jit = output(Command::new(strata).args(["run", "--jit"]).arg(&file));
    let interp = output(Command::new(strata).arg("run").arg(&file));
    assert_eq!(jit, interp);
    assert_eq!(jit.1, 101);
    // what doesn't compile yet is interpreted, and says why
    let f = d.join("lists.strata");
    save(&f, "fn main() -> () { print(matches(\"a1\", \"[a-z][0-9]\")) }\n");
    let (out, code) = output(Command::new(strata).args(["run", "--jit"]).arg(&f));
    assert_eq!(code, 0);
    assert!(out.contains("true\n") && out.contains("note: interpreted, not compiled: can't compile `main`: the built-in `matches` on (String, String) is not compiled yet"), "{out}");
}
