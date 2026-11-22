//! `std.ast.check` (the type checker in Strata) finds the errors `strata
//! check` finds (clashing definitions, unknown names), worded as it words them.

use std::io::Write;
use std::process::{Command, Stdio};

const STRATA: &str = env!("CARGO_BIN_EXE_strata");

const CHECKER: &str = r#"use std.fs.{Files, Disk}
use std.process.{Environment, Process}
use std.ast.binary.{decode_module}
use std.ast.check.{definition_errors, variant_errors, unknown_names}

fn main(disk: Disk, process: Process) {
  for a in process.args() {
    match disk.read_bytes(a) {
      Ok(bs) => match decode_module(bs) {
        Ok(ds) => {
          for e in definition_errors(ds) + variant_errors(ds) + unknown_names(ds) {
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
