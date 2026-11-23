//! The codebase tool in Strata (`compiler/store`, `compiler/codebase`)
//! against the Rust one: module files written byte for byte as strata
//! writes them (each definition's deps, hashes and sizes in the header),
//! and the commands (`init`, `log`) saying what they say.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[path = "common/compiler.rs"]
mod compiler;

const STRATA: &str = env!("CARGO_BIN_EXE_strata");

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("strata-codebase-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The module files of a package of the repository, sorted.
fn package_files(dir: &Path) -> Vec<String> {
    let mut out = vec![];
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                todo.push(p);
            } else if p.extension().is_some_and(|x| x == "strata") {
                out.push(p.to_string_lossy().into_owned());
            }
        }
    }
    out.sort();
    out
}

#[test]
fn module_files_are_written_as_strata_writes_them() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut wrong = vec![];
    for p in ["std", "core", "compiler", "examples/tasks", "examples/fuzzy", "examples/hello", "examples/wc", "examples/page", "examples/counter", "examples/todos"] {
        let mut args = vec!["store-check".to_string()];
        args.extend(package_files(&root.join(p)));
        let out = compiler::run(&args);
        for l in out.lines().filter(|l| !l.starts_with("ok ")) {
            wrong.push(format!("{p}: {l}"));
        }
        assert!(out.lines().filter(|l| l.starts_with("ok ")).count() == args.len() - 1 || !wrong.is_empty(), "{p}: {out}");
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Run in `dir`: strata, or the compiler package's `main`.
fn in_dir(dir: &Path, exe: &Path, args: &[&str], stdin: &str) -> (bool, String, String) {
    let mut c = Command::new(exe).args(args).current_dir(dir).env_remove("FORCE_COLOR").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    (o.status.success(), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}

#[test]
fn init_and_log_say_what_strata_says() {
    let ours = compiler::exe();
    let rust = Path::new(STRATA);
    // `init` in two folders named alike, one by each
    let a = scratch("a").join("shop");
    let b = scratch("b").join("shop");
    for d in [&a, &b] {
        std::fs::create_dir_all(d).unwrap();
    }
    let (ok, out_rust, _) = in_dir(&a, rust, &["init"], "");
    assert!(ok, "{out_rust}");
    let (ok, out_ours, err) = in_dir(&b, ours, &["init"], "");
    assert!(ok, "{err}");
    assert_eq!(out_ours, out_rust);
    for f in ["strata.toml", "strata.log"] {
        assert_eq!(std::fs::read(a.join(f)).unwrap(), std::fs::read(b.join(f)).unwrap(), "{f}");
    }
    // again: refused
    let (ok, _, err) = in_dir(&b, ours, &["init"], "");
    assert!(!ok && err.contains("already exists"), "{err}");
    // a log of edits: added, changed (interface or not), removed
    for (text, msg, rm) in [("pub fn f() -> Int { 1 }\n", "first", None), ("pub fn f() -> Int { 2 }\nfn g() -> Int { f() }\n", "second", None), ("pub fn f(x: Int) -> Int { x }\nfn g() -> Int { f(1) }\n", "third", None), ("", "fourth", Some("g"))] {
        let mut args = vec!["edit", "-m", msg, "--no-commit"];
        if let Some(r) = rm {
            args.extend(["--rm", r]);
        }
        let (ok, out, err) = in_dir(&a, rust, &args, text);
        assert!(ok, "{out}{err}");
    }
    let (_, want, _) = in_dir(&a, rust, &["log"], "");
    let (ok, got, err) = in_dir(&a, ours, &["log"], "");
    assert!(ok, "{err}");
    assert_eq!(got, want);
    assert!(want.contains("● third") && want.contains("~ interface") && want.contains("-            g"), "{want}");
    // outside a package
    let (ok, _, err) = in_dir(&std::env::temp_dir(), ours, &["log"], "");
    assert!(!ok && err.contains("no package here"), "{err}");
}
