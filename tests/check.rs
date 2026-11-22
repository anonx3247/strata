//! `std.ast.check` (the type checker in Strata) finds the errors `strata
//! check` finds (clashing definitions, unknown names,
//! wrong numbers of arguments, type mismatches), worded as it words them.

use std::io::Write;
use std::process::{Command, Stdio};

const STRATA: &str = env!("CARGO_BIN_EXE_strata");

const CHECKER: &str = r#"use std.fs.{Files, Disk}
use std.process.{Environment, Process}
use std.ast.binary.{decode_module}
use std.ast.check.{definition_errors, variant_errors, expression_errors, type_errors}

fn main(disk: Disk, process: Process) {
  for a in process.args() {
    match disk.read_bytes(a) {
      Ok(bs) => match decode_module(bs) {
        Ok(ds) => {
          for e in definition_errors(ds) + variant_errors(ds) + expression_errors(ds) + type_errors(ds) {
            print(f"error in {e.0}: {e.1}")
          }
        },
        Err(e) => print(e),
      },
      Err(_) => print("unreadable"),
    }
  }
}
"#;

const CLASHES: &str = r#"type Line = { a: Int }
type E =
  | Line(Int)
  | Y
type Option = { b: Int }
fn main() { print(1) }
"#;

const ARITY: &str = r#"type Pt = { x: Int, y: Int }
type Sh =
  | Sq(Int)
  | Rc(Int, Int)
fn f(a: Int, b: Int) -> Int { a + b }
fn g(x: Int) -> Int { f(f(x)) + zz }
fn h() -> Sh { Rc(f(1), yy, 3) }
fn m() -> Sh { Sq }
fn n() -> Pt { Pt(1, 2) }
fn o(f: Int) -> Int { f }
fn main() { print(g(1)) }
"#;

const TYPES: &str = r#"fn f(a: Int) -> Int { a + "x" }
fn g(a: Int) -> String { a }
fn h(s: String) -> Int { f(s) }
fn k() -> Bool { 1 < 2.5 }
fn m() -> Int { x := true
 x + 1 }
fn n() -> List[Int] { [1, "a"] }
fn o(b: Bool) -> Int { if b { 1 } else { "no" } }
fn p() -> Float { 1 }
fn main() { print(1) }
"#;

const STATEMENTS: &str = r#"fn g(x: Int) -> Int { x }
fn a() { g(1) }
fn b(c: Bool) -> Int { if c { 1 } }
fn d(c: Bool) { if c { g(1) } }
fn e() -> Int {
  mut s := 0
  s = "x"
  while 1 { s += 1 }
  assert 2
  -true
  !3
  return "r"
}
fn h(c: Bool) -> Int { match c { true => 1, false => "f" } }
fn k(c: Bool) -> Int {
  let y: String = 3
  if c { [1] } else { "s" }
  0
}
fn main() { print(1) }
"#;

const UNKNOWN: &str = r#"type Pt = { x: Int, y: Int }
fn f(a: Int) -> Int {
  b := a + c
  mut s := 0
  s += b
  if b > 0 { d } else { a + s }
}
fn g(xs: List[Int]) -> Int {
  ys := xs.map(x -> x + y)
  for z in xs { print(z + w) }
  zs := [u * 2 for u in xs where u > v]
  p := Pt { x: 1, y: r }
  print(f"{p.x} {nowhere}")
  match ys { [h, ..rest] => h + len(rest) + k, _ => q }
}
fn main() { print(f(1) + g([1]) + nope(2)) }
"#;

fn save(file: &std::path::Path, text: &str) {
    let mut c = Command::new(STRATA).args(["--file", &file.to_string_lossy(), "edit"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    c.wait_with_output().unwrap();
}

#[test]
fn clashes_as_in_rust() {
    same_errors("clashes", CLASHES, &["defined twice", "already defined in the prelude", "two `impl", "already implemented", "a variant is a type of its own"]);
}

#[test]
fn unknown_names_as_in_rust() {
    same_errors("unknown", UNKNOWN, &["unknown variable"]);
}

#[test]
fn arity_as_in_rust() {
    same_errors("arity", ARITY, &["unknown variable", "expects", "needs"]);
}

#[test]
fn types_as_in_rust() {
    same_errors("types", TYPES, &["expected `", "Int or Float"]);
}

#[test]
fn statement_types_as_in_rust() {
    same_errors("statements", STATEMENTS, &["expected `", "Int or Float"]);
}

/// `std.ast.check`'s errors on `text` are `strata check`'s lines holding one of `keys`.
fn same_errors(name: &str, text: &str, keys: &[&str]) {
    let dir = std::env::temp_dir().join(format!("strata-check-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (checker, prog) = (dir.join("check.strata"), dir.join(format!("{name}.strata")));
    save(&checker, CHECKER);
    save(&prog, text);
    let got = Command::new(STRATA).args(["--file", &checker.to_string_lossy(), "run", "--", &prog.to_string_lossy()]).output().unwrap();
    let got = String::from_utf8_lossy(&got.stdout).to_string();
    let o = Command::new(STRATA).args(["--file", &prog.to_string_lossy(), "check"]).output().unwrap();
    let all = String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr);
    let want: String = all.lines().filter(|l| keys.iter().any(|k| l.contains(k))).map(|l| format!("{l}\n")).collect();
    assert!(!want.is_empty(), "{all}");
    assert_eq!(got, want);
    let _ = std::fs::remove_dir_all(&dir);
}
