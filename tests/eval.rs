//! `std.ast.eval` runs programs from their stored trees, in Strata, as the
//! compiled programs run them: for each program of tests/build.rs below, it
//! prints what the compiled program printed, ends with the same fault or
//! error, and gives the same exit code (the outputs recorded in
//! tests/data/build/expected).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const STRATA: &str = env!("CARGO_BIN_EXE_strata");

/// The checker, run as `PRELUDE FILE...`: each file's program run with the
/// prelude's definitions, its output then `--- exit N`, then `===`.
const CHECKER: &str = r#"use std.fs.{Files, Disk}
use std.process.{Environment, Process}
use std.ast.binary.{read}
use std.ast.eval.{run}

fn decls_of(disk: Disk, f: String) -> List[std.ast.Decl] {
  match disk.read_bytes(f) {
    Ok(bytes) => match read(bytes) {
      Ok(img) => img.decls,
      Err(e) => {
        print(f"error: {f}: {e}")
        []
      },
    },
    Err(e) => {
      print(f"error: {f}: {e}")
      []
    },
  }
}

fn main(disk: Disk, process: Process) {
  args := process.args()
  prelude := decls_of(disk, get(args, 0) ?? "")
  for f in drop(args, 1) {
    r := run(prelude + decls_of(disk, f), ["one", "two words"])
    for l in r.stdout {
      print(l)
    }
    for l in r.stderr {
      print(l)
    }
    print(f"--- exit {r.code}")
    print("===")
  }
}
"#;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("strata-eval-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn have_clang() -> bool {
    let cc = std::env::var("STRATA_CC").unwrap_or_else(|_| "clang".into());
    Command::new(cc).arg("--version").output().is_ok_and(|o| o.status.success())
}

/// Store a program written as text at `file` (a module file).
fn save(file: &Path, text: &str) {
    let mut c = Command::new(STRATA).args(["--file", &file.to_string_lossy(), "edit"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success(), "{}: {}", file.display(), String::from_utf8_lossy(&o.stderr));
}

fn strata(args: &[&str]) {
    let o = Command::new(STRATA).args(args).output().unwrap();
    assert!(o.status.success(), "strata {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

/// The programs of tests/build.rs whose recorded outputs the evaluator
/// reproduces: (the expected output's key, the program's text).
const CASES: &[(&str, &str)] = &[
    ("faults-early_return", "fn sign(n: Int) -> Int { if n < 0 { return -1 } else { 0 } }\nfn main() -> () { print(sign(-5)) print(sign(5)) }"),
    ("faults-requires", "fn half(n: Int) -> Int requires n % 2 == 0 { n / 2 }\nfn main() -> () { print(half(4)) print(half(3)) }"),
    ("faults-ensures", "fn f(x: Int) -> Int ensures it > x { x }\nfn main() -> () { print(f(2)) }"),
    ("faults-assert", "fn main() { s := \"a\\\"b\" assert s + \"!\" == \"x\" }"),
    ("faults-assignment", "fn main() { mut x: Int where it < 5 = 0 while true { x = x + 1 } }"),
    ("faults-division", "fn main() { print(-7 / 2) print(-7 % 2) print(7 / -2) print(1 / 0) }"),
    ("faults-floats", "fn main() { print(0.1 + 0.2) print(1000000000000000000000.0) print(0.00000015) print(100.0) print(-5.5 % 2.0) print(1.0 / 0.0) }"),
    ("faults-exit_code", "fn main() -> Int { 3 }"),
    (
        "faults-loops",
        "fn g(n: Int) -> Int { mut i := 0 while i < 10 { i = i + 1 if i == n { break } if i % 2 == 0 { continue } } i }\n\
         fn pick(b: Bool) -> String { match b { true => \"yes\", false => return \"no\" } }\n\
         fn main() { print(g(4)) print(g(40)) print(pick(true) + pick(false)) print(f\"{1.5} {true} {\"q\"} {len(\"héllo\".code_points())}\") }",
    ),
    ("data-field_refinement", "type Quantity = Int where 1 <= it && it <= 99\ntype Line = { sku: String, qty: Quantity }\nfn main() -> () { mut l := Line { sku: \"a\", qty: 5 } l.qty = 0 }"),
    (
        "data-tree",
        "type Tree[T] =\n  | Leaf\n  | Node(Tree[T], T, Tree[T])\n\
         fn insert(t: Tree[Int], x: Int) -> Tree[Int] { match t { Leaf => Node(Leaf, x, Leaf), Node(l, v, r) => if x < v { Node(insert(l, x), v, r) } else { Node(l, v, insert(r, x)) } } }\n\
         fn size[T](t: Tree[T]) -> Int { match t { Leaf => 0, Node(l, _, r) => size(l) + 1 + size(r) } }\n\
         fn main() { mut t: Tree[Int] = Leaf for i in range(0, 10) { t = insert(t, (i * 7) % 10) } print(size(t)) print(insert(insert(Leaf, 2), 1)) }",
    ),
    ("data-own_methods", "type Item = { name: String, price: Int }\nimpl Item {\n  fn new(name: String) -> Item { Item { name: name, price: 0 } }\n  fn label(self) -> String { f\"{self.name}: {self.price}\" }\n}\nfn main() -> () { print(Item.new(\"cup\").label()) }"),
    ("data-refined_argument", "fn f(o: Option[Positive]) -> Int { o ?? 0 }\nfn main() -> () { print(f(Some(3))) let x: Option[Int] = Some(-2) print(f(x)) }"),
    ("data-main_err", "fn main() -> Result[(), String] { print(\"working\") Err(\"it broke\") }"),
    (
        "data-errors",
        "type E =\n  | NotFound\n  | Bad { why: String }\n\
         fn g(n: Int) -> Result[Int, E] { if n == 0 { Err(NotFound) } else if n < 0 { Err(Bad { why: \"neg\" }) } else { Ok(n) } }\n\
         fn h(n: Int) -> Result[Int, E] { v := g(n)! Ok(v * 2) }\n\
         fn main() { print(h(3)) print(h(0)) print(h(-1)) print(h(0) == Err(NotFound)) (x, y) := (5, \"s\") print(f\"{x}{y}\") }",
    ),
];

/// The stored programs of tests/data/build.
const FILES: &[&str] = &["numbers", "lists", "data"];

#[test]
fn programs_run_as_compiled() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let d = scratch("programs");
    let checker = d.join("check.strata");
    save(&checker, CHECKER);
    let exe = d.join("check");
    strata(&["build", &checker.to_string_lossy(), "-o", &exe.to_string_lossy()]);
    let prelude = d.join("prelude.strata");
    strata(&["pack", "--prelude", &prelude.to_string_lossy()]);
    let mut programs: Vec<(String, PathBuf)> = FILES.iter().map(|f| (f.to_string(), root.join("tests/data/build").join(format!("{f}.strata")))).collect();
    for (key, text) in CASES {
        let f = d.join(format!("{key}.strata"));
        save(&f, text);
        programs.push((key.to_string(), f));
    }
    let mut c = Command::new(&exe);
    c.arg(&prelude).args(programs.iter().map(|p| &p.1));
    let o = c.output().unwrap();
    assert!(o.status.success(), "the checker failed: {}", String::from_utf8_lossy(&o.stderr));
    let out = String::from_utf8(o.stdout).unwrap();
    let runs: Vec<&str> = out.split("===\n").collect();
    let mut wrong = vec![];
    for ((key, _), got) in programs.iter().zip(&runs) {
        // the recorded form: the output, a blank line, then the exit code
        let (text, exit) = got.rsplit_once("--- exit ").unwrap_or((got, "?\n"));
        let got = format!("{text}\n--- exit {exit}");
        let want = std::fs::read_to_string(root.join("tests/data/build/expected").join(format!("{key}.out"))).unwrap();
        if got != want {
            wrong.push(format!("{key}:\n--- std.ast.eval\n{got}--- compiled\n{want}"));
        }
    }
    assert_eq!(runs.len(), programs.len() + 1, "{out}");
    assert!(wrong.is_empty(), "{} of {} programs differ:\n{}", wrong.len(), programs.len(), wrong.join("\n"));
}
