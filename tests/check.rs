//! `std.ast.check` (the type checker in Strata, step 1) finds the clashing
//! definitions `strata check` finds, worded as it words them.

use std::io::Write;
use std::process::{Command, Stdio};

const STRATA: &str = env!("CARGO_BIN_EXE_strata");

const CHECKER: &str = r#"use std.fs.{Files, Disk}
use std.process.{Environment, Process}
use std.ast.binary.{decode_module}
use std.ast.check.{definition_errors}

fn main(disk: Disk, process: Process) {
  for a in process.args() {
    match disk.read_bytes(a) {
      Ok(bs) => match decode_module(bs) {
        Ok(ds) => {
          for e in definition_errors(ds) {
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

const CLASHES: &str = r#"type X = { a: Int }
fn f() -> Int { 1 }
fn f() -> Int { 2 }
type Option = { b: Int }
trait Show { fn show(self) -> String }
impl X { fn one(self) -> Int { 1 } }
impl X { fn two(self) -> Int { 2 } }
impl Show for X { fn show(self) -> String { "x" } }
impl Show for X { fn show(self) -> String { "y" } }
type X = { c: Int }
fn main() { print(1) }
"#;

fn save(file: &std::path::Path, text: &str) {
    let mut c = Command::new(STRATA).args(["--file", &file.to_string_lossy(), "edit"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    c.wait_with_output().unwrap();
}

#[test]
fn clashes_as_in_rust() {
    let dir = std::env::temp_dir().join(format!("strata-check-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (checker, prog) = (dir.join("check.strata"), dir.join("clashes.strata"));
    save(&checker, CHECKER);
    save(&prog, CLASHES);
    let got = Command::new(STRATA).args(["--file", &checker.to_string_lossy(), "run", "--", &prog.to_string_lossy()]).output().unwrap();
    let got = String::from_utf8_lossy(&got.stdout).to_string();
    let o = Command::new(STRATA).args(["--file", &prog.to_string_lossy(), "check"]).output().unwrap();
    let all = String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr);
    let want: String = all.lines().filter(|l| ["defined twice", "already defined in the prelude", "two `impl", "already implemented"].iter().any(|k| l.contains(k))).map(|l| format!("{l}\n")).collect();
    assert!(!want.is_empty(), "{all}");
    assert_eq!(got, want);
    let _ = std::fs::remove_dir_all(&dir);
}
