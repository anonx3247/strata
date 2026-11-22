//! `std.ast.builtins` holds the signatures of `src/types.rs`'s `BUILTINS`:
//! printed as text, they are `strata builtins`' lines, in the same order.

use std::io::Write;
use std::process::{Command, Stdio};

const STRATA: &str = env!("CARGO_BIN_EXE_strata");

const PRINTER: &str = r#"use std.ast.builtins.{signatures}
use std.ast.text.{type_text}

fn main() {
  for s in signatures() {
    tps := if len(s.type_params) == 0 { "" } else { "[" + join(s.type_params, ",") + "]" }
    print(s.name + tps + "(" + join(s.params.map(p -> type_text(p)), ", ") + ") -> " + type_text(s.ret))
  }
}
"#;

#[test]
fn builtins_as_in_rust() {
    let dir = std::env::temp_dir().join(format!("strata-builtins-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("printer.strata");
    let mut c = Command::new(STRATA).args(["--file", &file.to_string_lossy(), "edit"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(PRINTER.as_bytes()).unwrap();
    c.wait_with_output().unwrap();
    let got = Command::new(STRATA).args(["--file", &file.to_string_lossy(), "run"]).output().unwrap();
    let got = String::from_utf8_lossy(&got.stdout).to_string();
    let o = Command::new(STRATA).arg("builtins").output().unwrap();
    let all = String::from_utf8_lossy(&o.stdout).to_string();
    let (_, listed) = all.split_once("// builtin functions\n\n").expect("strata builtins lists the builtin functions");
    assert!(listed.lines().count() > 50, "{all}");
    assert_eq!(got, listed);
}
