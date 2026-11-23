//! `compiler/smt` (the compiler package's `smt` command), the verifier's
//! SMT encoding written in Strata: for each function it covers, the
//! solver's answer to each obligation must be the one `strata verify
//! --obligations` gets (proven, refuted or unknown), obligation for
//! obligation. A function using what it doesn't model yet says `not
//! covered` and is left out of the comparison.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const STRATA: &str = env!("CARGO_BIN_EXE_strata");

#[path = "common/compiler.rs"]
mod compiler;

fn have_z3() -> bool {
    Command::new("z3").arg("--version").output().is_ok_and(|o| o.status.success())
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("strata-smt-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Store a program written as text at `file` (a module file).
fn save(file: &Path, text: &str) {
    let mut c = Command::new(STRATA).args(["--file", &file.to_string_lossy(), "edit"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success(), "{}: {}", file.display(), String::from_utf8_lossy(&o.stderr));
}

/// Each function's lines (`fn NAME`, then its obligations' outcomes).
fn functions(out: &str) -> Vec<(String, Vec<String>)> {
    let mut fns: Vec<(String, Vec<String>)> = vec![];
    for l in out.lines() {
        if let Some(n) = l.strip_prefix("fn ") {
            fns.push((n.to_string(), vec![]));
        } else if let (Some(line), Some(f)) = (l.strip_prefix("  "), fns.last_mut()) {
            f.1.push(line.to_string());
        }
    }
    fns
}

/// The script at `file` through both verifiers: the functions
/// `compiler/smt` covers, each with its outcomes from Rust and from Strata.
fn both(file: &Path, cache: &Path) -> (Vec<(String, Vec<String>, Vec<String>)>, usize) {
    let o = Command::new(STRATA).args(["--file", &file.to_string_lossy(), "verify", "--obligations"]).output().unwrap();
    assert!(o.status.success(), "{}: strata verify --obligations: {}", file.display(), String::from_utf8_lossy(&o.stderr));
    let rust = functions(&String::from_utf8_lossy(&o.stdout));
    let o = Command::new(compiler::exe()).arg("smt").arg(file).env("STRATA_CACHE", cache).output().unwrap();
    assert!(o.status.success(), "{}: compiler smt: {}", file.display(), String::from_utf8_lossy(&o.stderr));
    let ours = functions(&String::from_utf8_lossy(&o.stdout));
    let all = ours.len();
    let covered = ours
        .into_iter()
        .filter(|(_, ls)| !ls.first().is_some_and(|l| l.starts_with("not covered")))
        .map(|(n, ls)| {
            let r = rust.iter().find(|f| f.0 == n).map(|f| f.1.clone()).unwrap_or_else(|| panic!("{}: `{n}` is not among Rust's functions", file.display()));
            (n, r, ls)
        })
        .collect();
    (covered, all)
}

/// The functions covered (all agreeing), and all of them.
fn same(file: &Path, cache: &Path) -> (usize, usize) {
    let (fns, all) = both(file, cache);
    for (n, r, s) in &fns {
        assert_eq!(r, s, "{}: `{n}`: Rust's outcomes, then compiler/smt's", file.display());
    }
    (fns.len(), all)
}

/// Each step's program: every function of it is covered, and agrees.
/// Step 1: integers, booleans and strings, their refinements, operators,
/// `if`, blocks, variables, `assert`, `return` and calls through contracts.
/// Step 2: records, sums (`Option`, `Result`), tuples and distinct types as
/// datatypes; constructors, fields, updates, `match` and patterns, `is`,
/// `!` / `?`, `except` / `??`.
#[test]
fn smt_outcomes_as_in_rust() {
    if !have_z3() {
        eprintln!("skipped: no z3");
        return;
    }
    let dir = scratch("steps");
    for (name, text) in [("step1", include_str!("data/smt/step1.st")), ("step2", include_str!("data/smt/step2.st"))] {
        let file = dir.join(format!("{name}.strata"));
        save(&file, text);
        let (covered, all) = same(&file, &dir);
        assert_eq!(covered, all, "{name}: every function is covered");
    }
}

/// The repository's scripts: the functions `compiler/smt` covers agree.
#[test]
fn smt_outcomes_on_scripts_as_in_rust() {
    if !have_z3() {
        eprintln!("skipped: no z3");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("scripts");
    let mut covered = 0;
    for d in ["examples", "tests/data"] {
        let mut files: Vec<PathBuf> = std::fs::read_dir(root.join(d)).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "strata")).collect();
        files.sort();
        for f in files {
            covered += same(&f, &dir).0;
        }
    }
    assert!(covered > 0, "no function of the scripts is covered");
}
