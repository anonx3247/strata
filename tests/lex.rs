//! `compiler/lex` (the compiler package's `lex` command) makes the tokens
//! and comments `src/lexer.rs` makes, and refuses what it refuses with the same message: on the text of every
//! module file in the repository (as `strata render` writes it) and on
//! hand-written text with what rendering never produces (f-strings with
//! nested strings, holes, big integers, CRLF, tabs, Unicode names, errors).
//! Both sides print the tokens as `strata lex` does.

use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "common/compiler.rs"]
mod compiler;

const STRATA: &str = env!("CARGO_BIN_EXE_strata");

/// Text rendering never makes, each with what it tests.
const CASES: &[(&str, &str)] = &[
    (
        "fstrings",
        "fn f() { x := f\"a {g(\"}\", [1, {2}])} b {{c}} \\\"q\\\" {y:>8.2}\"\n  ?hole + 99999999999999999999 + 1_000 + 1.5 + 2_0.0_5 + xs.0.1 + 1..n + 1..=3\n  s := \"multi\nline \\d \\n \\t \\\\ end\" // trailing\n\t/// doc  \n  // own line\n  é_ü := Ωmega\n  a ==> b !! c ?? d ... e $f <<g>> h\n}\n",
    ),
    ("crlf", "x := 1\r\ny := \"a\r\nb\"\r\n"),
    ("numbers", "x := 1.5.6 + 3. + 9223372036854775807 + 9223372036854775808"),
    ("empty", ""),
    ("unicode", "名前 := 1 + x² + ٣٤"),
    ("unterminated_code", "f\"open {x"),
    ("lone_brace", "f\"a } b\""),
    ("empty_code", "f\"a {  } b\""),
    ("unterminated_string", "x := \"never closed"),
    ("unexpected", "x := 1 @ 2"),
    ("unterminated_fstring", "f\"unterminated"),
    ("hash", "x # y"),
];

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("strata-lex-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(args: &[&str]) -> String {
    let o = Command::new(STRATA).args(args).output().unwrap();
    assert!(o.status.success(), "strata {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}

/// Every module file in the repository: std, the examples, test data and
/// the compiled prelude.
fn module_files() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = vec![];
    let mut todo = vec![root.join("std"), root.join("compiler"), root.join("examples"), root.join("tests/data"), root.join("src/llvm")];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                if !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
                    todo.push(p);
                }
            } else if p.extension().is_some_and(|x| x == "strata") && std::fs::read(&p).unwrap().starts_with(b"STRATA\x01\x00") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn lexes_as_in_rust() {
    let dir = scratch("all");
    // the text of each module file, as rendered, and the cases
    let mut texts = vec![];
    for (i, f) in module_files().iter().enumerate() {
        let text = run(&["--file", &f.to_string_lossy(), "render"]);
        let p = dir.join(format!("module{i}.st"));
        std::fs::write(&p, text).unwrap();
        texts.push((f.display().to_string(), p));
    }
    assert!(texts.len() > 30);
    for (name, text) in CASES {
        let p = dir.join(format!("{name}.st"));
        std::fs::write(&p, text).unwrap();
        texts.push((name.to_string(), p));
    }
    let paths: Vec<String> = texts.iter().map(|t| t.1.to_string_lossy().into_owned()).collect();
    let mut args = vec!["lex"];
    args.extend(paths.iter().map(|s| s.as_str()));
    let got = compiler::run(&args);
    let mut wrong = vec![];
    for (i, (name, p)) in texts.iter().enumerate() {
        let start = format!("=== {}\n", p.display());
        let from = got.find(&start).unwrap_or_else(|| panic!("no tokens for {name}")) + start.len();
        let until = paths.get(i + 1).and_then(|n| got[from..].find(&format!("=== {n}\n")).map(|k| from + k)).unwrap_or(got.len());
        let want = run(&["lex", &p.to_string_lossy()]);
        let strata = &got[from..until];
        if strata != want {
            let (g, w): (Vec<&str>, Vec<&str>) = (strata.lines().collect(), want.lines().collect());
            let at = g.iter().zip(&w).position(|(a, b)| a != b).unwrap_or(g.len().min(w.len()));
            wrong.push(format!("{name}, line {}:\n  compiler/lex: {}\n  Rust:         {}", at + 1, g.get(at).unwrap_or(&"(none)"), w.get(at).unwrap_or(&"(none)")));
        }
    }
    assert!(wrong.is_empty(), "{} of {} texts lex differently:\n{}", wrong.len(), texts.len(), wrong.join("\n"));
    let _ = std::fs::remove_dir_all(&dir);
}
