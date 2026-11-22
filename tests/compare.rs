//! `--compare` on the commands the compiler written in Strata implements
//! (`lex`, `parse`, `render`, `run`, `build`; `check` is in tests/check.rs):
//! on a small script both implementations agree, so each exits 0.

use std::path::PathBuf;
use std::process::Command;

const STRATA: &str = env!("CARGO_BIN_EXE_strata");
const PROGRAM: &str = "fn twice(n: Int) -> Int {\n  n * 2\n}\n\nfn main() {\n  print(\"hi\")\n  print(twice(21))\n}\n";

fn have_clang() -> bool {
    let cc = std::env::var("STRATA_CC").unwrap_or_else(|_| "clang".into());
    Command::new(cc).arg("--version").output().is_ok_and(|o| o.status.success())
}

/// The script as text and stored, in a fresh folder named after the test.
fn files(name: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("strata-compare-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (text, stored) = (dir.join("p.st"), dir.join("p.strata"));
    std::fs::write(&text, PROGRAM).unwrap();
    let mut c = Command::new(STRATA).args(["--file", &stored.to_string_lossy(), "edit"]).stdin(std::process::Stdio::piped()).spawn().unwrap();
    std::io::Write::write_all(c.stdin.as_mut().unwrap(), PROGRAM.as_bytes()).unwrap();
    assert!(c.wait().unwrap().success());
    (text, stored)
}

/// `strata ARGS --compare` exits 0 and says the two agree.
fn agrees(args: &[&str]) {
    let cache = std::env::temp_dir().join("strata-compare-test-cache");
    let o = Command::new(STRATA).args(args).arg("--compare").env("STRATA_CACHE", &cache).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(0), "{args:?}: {out}");
    assert!(out.contains("compare: they agree (") && !out.contains("only in"), "{out}");
}

#[test]
fn compare_lex() {
    if !have_clang() {
        return eprintln!("skipped: no clang");
    }
    let (text, _) = files("lex");
    agrees(&["lex", &text.to_string_lossy()]);
}

#[test]
fn compare_parse() {
    if !have_clang() {
        return eprintln!("skipped: no clang");
    }
    let (text, _) = files("parse");
    agrees(&["parse", &text.to_string_lossy()]);
    agrees(&["parse", "--stored", &text.to_string_lossy()]);
}

#[test]
fn compare_render() {
    if !have_clang() {
        return eprintln!("skipped: no clang");
    }
    let (_, stored) = files("render");
    agrees(&["--file", &stored.to_string_lossy(), "render"]);
}

#[test]
fn compare_run() {
    if !have_clang() {
        return eprintln!("skipped: no clang");
    }
    let (_, stored) = files("run");
    agrees(&["--file", &stored.to_string_lossy(), "run"]);
}

#[test]
fn compare_build() {
    if !have_clang() {
        return eprintln!("skipped: no clang");
    }
    let (_, stored) = files("build");
    agrees(&["build", &stored.to_string_lossy()]);
}
