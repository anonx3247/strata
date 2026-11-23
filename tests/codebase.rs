//! The codebase tool in Strata (`compiler/store`, `compiler/codebase`)
//! against the Rust one: module files written byte for byte as strata
//! writes them (each definition's deps, hashes and sizes in the header),
//! and the commands (`init`, `log`, the edits, the views, `diff`, `holes`,
//! `pack`, and those for git) saying and writing what they say and write.

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

/// One step done by strata in `a` and by the compiler package in `b` (two
/// copies of a package): the same output and exit code, the same module
/// files, byte for byte, and the same log (but each entry's id and time).
fn both(a: &Path, b: &Path, args: &[&str], stdin: &str, editor: Option<&Path>) {
    both_as(a, b, args, args, stdin, editor)
}

/// `both`, with strata given `rust_args` and the compiler package `args`
/// (a command strata names otherwise: `render`, which is `view` in Strata).
fn both_as(a: &Path, b: &Path, rust_args: &[&str], args: &[&str], stdin: &str, editor: Option<&Path>) {
    let home = env!("CARGO_MANIFEST_DIR");
    let run = |dir: &Path, exe: &Path, args: &[&str]| {
        let mut c = Command::new(exe);
        // (`sync` has git run this executable's `merge-file`: strata names
        // itself, the compiler package reads `STRATA_EXE`)
        c.args(args).current_dir(dir).env("STRATA_HOME", home).env("STRATA_EXE", exe).env_remove("FORCE_COLOR").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(e) = editor {
            c.env("EDITOR", e).env_remove("VISUAL");
        }
        let mut c = c.spawn().unwrap();
        c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
        let o = c.wait_with_output().unwrap();
        (o.status.code(), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
    };
    let want = run(a, Path::new(STRATA), rust_args);
    let got = run(b, compiler::exe(), args);
    assert_eq!(got, want, "{args:?}");
    let files = |d: &Path| -> Vec<(PathBuf, Vec<u8>)> { package_files(d).into_iter().map(|f| (PathBuf::from(&f).strip_prefix(d).unwrap().to_path_buf(), std::fs::read(&f).unwrap())).collect() };
    assert!(files(a) == files(b), "{args:?}: the module files differ");
    let log = |d: &Path| -> String {
        let t = std::fs::read_to_string(d.join("strata.log")).unwrap_or_default();
        t.lines().map(|l| l.split_once(",\"message\"").map_or(l.to_string(), |(_, r)| r.to_string())).collect::<Vec<_>>().join("\n")
    };
    assert_eq!(log(b), log(a), "{args:?}: the logs differ");
}

fn two_packages(name: &str) -> (PathBuf, PathBuf) {
    let (a, b) = (scratch(&format!("{name}-a")).join("shop"), scratch(&format!("{name}-b")).join("shop"));
    for d in [&a, &b] {
        std::fs::create_dir_all(d).unwrap();
        assert!(in_dir(d, Path::new(STRATA), &["init"], "").0);
    }
    (a, b)
}

#[test]
fn edits_pending_changes_renames_and_moves_as_strata_makes_them() {
    let (a, b) = two_packages("edits");
    let step = |args: &[&str], stdin: &str| both(&a, &b, args, stdin, None);
    // edits: added, changed (interface or not), a module, imports, rejected, removed
    step(&["edit", "-m", "orders", "orders", "--no-commit"], "/// The total.\npub type Order = { total: Int, items: List[Int] }\n\npub type State =\n  | Open\n  | Shipped(Int)\n\npub fn size(o: Order) -> Int { len(o.items) }\n\nfn helper() -> Int { 1 }\n");
    step(&["edit", "-m", "main", "--no-commit"], "from orders use Order, size, Open, Shipped, State\n\nfn count(o: Order) -> Int { size(o) + o.total }\n\nfn st(s: State) -> Int { match s { Open => 0, Shipped(n) => n } }\n\npub fn f() -> Int { 1 }\n\nfn g() -> Int { f() + 1 }\n");
    step(&["edit", "-m", "impl", "--no-commit"], "pub fn f() -> Int { 2 }\n");
    step(&["edit", "-m", "bad", "--no-commit"], "fn k() -> Int { \"x\" }\n");
    // a pending change: holes, status, filled, done; a variant removed; abort
    step(&["edit", "-m", "sig", "--no-commit"], "pub fn f(x: Int) -> Int { x }\n");
    step(&["status"], "");
    step(&["edit", "-m", "fill", "--no-commit"], "fn g() -> Int { f(2) + 1 }\n");
    step(&["done", "--no-commit"], "");
    step(&["edit", "-m", "two", "--no-commit"], "pub fn f(x: Int, y: Int) -> Int { x + y }\n");
    step(&["done", "--keep-holes", "-m", "kept", "--no-commit"], "");
    step(&["edit", "-m", "again", "--no-commit"], "pub fn f() -> Int { 3 }\n");
    step(&["abort"], "");
    // renames (a function, a type, a field, refused ones) and moves
    step(&["rename", "size", "length", "--no-commit"], "");
    step(&["rename", "Order", "Purchase", "--no-commit"], "");
    step(&["rename", "orders.Purchase.total", "sum", "--no-commit"], "");
    step(&["rename", "nothing", "x", "--no-commit"], "");
    step(&["rename", "count", "Count", "--no-commit"], "");
    step(&["move", "length", "main", "--no-commit"], "");
    step(&["move", "helper", "main", "--no-commit"], "");
    step(&["move", "count", "orders", "--no-commit"], "");
    step(&["edit", "--rm", "count", "-m", "removed", "--no-commit"], "");
    // `add`, and the editor (a script standing for one)
    for d in [&a, &b] {
        std::fs::write(d.join("defs.st"), "pub fn h() -> Int { 1 }\n").unwrap();
    }
    step(&["add", "defs.st", "lib", "-m", "imported", "--no-commit"], "");
    let editor = a.parent().unwrap().join("editor.sh");
    std::fs::write(&editor, "#!/bin/sh\nfor f in \"$@\"; do sed -i.bak \"s/^  1$/  7/\" \"$f\"; rm -f \"$f.bak\"; done\n").unwrap();
    std::process::Command::new("chmod").arg("+x").arg(&editor).status().unwrap();
    both(&a, &b, &["edit", "lib", "--editor", "--no-commit"], "", Some(&editor));
    step(&["log"], "");
}

/// A copy of `from` at `to` (a package, or a repository), byte for byte.
fn copy(from: &Path, to: &Path) {
    let _ = std::fs::remove_dir_all(to);
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    assert!(Command::new("cp").arg("-R").arg(from).arg(to).status().unwrap().success());
}

/// A package with a history, built by strata, then the same package twice:
/// each read-only command run on one copy by strata and on the other by the
/// compiler package.
#[test]
fn views_diff_holes_and_pack_as_strata_shows_them() {
    let rust = Path::new(STRATA);
    let a = scratch("views-a").join("shop");
    std::fs::create_dir_all(&a).unwrap();
    assert!(in_dir(&a, rust, &["init"], "").0);
    for (module, msg, text) in [
        ("orders", "orders", "/// The total.\npub type Order = { total: Int, items: List[Int] }\n\npub type State =\n  | Open\n  | Shipped(Int)\n\npub fn size(o: Order) -> Int\n  ensures it >= 0\n{ len(o.items) }\n"),
        ("main", "main", "from orders use Order, size\n\nfn count(o: Order) -> Int { size(o) + o.total }\n\nfn later(x: Int, s: String) -> Int {\n  y := x + 1\n  ?todo\n}\n"),
        ("orders", "state", "pub type State =\n  | Open\n  | Shipped(Int)\n  | Lost\n\npub fn size(o: Order) -> Int\n  ensures it >= 0\n  ensures it <= len(o.items)\n{ len(o.items) }\n"),
    ] {
        let (ok, out, err) = in_dir(&a, rust, &["edit", module, "-m", msg, "--no-commit"], text);
        assert!(ok, "{out}{err}");
    }
    let (ok, out, err) = in_dir(&a, rust, &["rename", "size", "length", "--no-commit"], "");
    assert!(ok, "{out}{err}");
    let b = scratch("views-b").join("shop");
    copy(&a, &b);
    let same = |args: &[&str]| both(&a, &b, args, "", None);
    // a module as text: strata's `render`, Strata's `view`
    both_as(&a, &b, &["render", "orders"], &["view", "orders"], "", None);
    both_as(&a, &b, &["render", "main"], &["view", "main"], "", None);
    for args in [&["view"][..], &["view", "--level", "0"], &["view", "--level", "2"], &["view", "orders.length"], &["view", "orders.*", "--level", "2"], &["interface", "orders"], &["header", "orders.strata"], &["diff"], &["diff", "--step", "2"], &["diff", "--step", "3"], &["diff", "--from", "1", "--to", "2"], &["holes"], &["pack", "shop.strata"]] {
        same(args);
    }
    assert!(std::fs::read(a.join("shop.strata")).unwrap() == std::fs::read(b.join("shop.strata")).unwrap(), "the bundles differ");
}

/// `git ARGS` in `dir`, with fixed dates (so two copies make the same commits).
fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git").args(args).current_dir(dir).env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00").env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00").output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

/// A repository with a package on two branches (one renames a function the
/// other's new code calls, in another module), built by strata and copied:
/// `diff` against git, `upgrade`, `sync` (git merging with each one's
/// `merge-file`, renames followed, the merge checked and committed), and a
/// conflict left as text, then `resolve`d.
#[test]
fn git_commands_as_strata_runs_them() {
    let rust = Path::new(STRATA);
    let a = scratch("git-a").join("repo");
    let app = a.join("app");
    std::fs::create_dir_all(&app).unwrap();
    git(&a, &["init", "-q", "-b", "main"]);
    git(&a, &["config", "user.email", "t@t"]);
    git(&a, &["config", "user.name", "t"]);
    let edit = |module: &str, msg: &str, text: &str| {
        let (ok, out, err) = in_dir(&app, rust, &["edit", module, "-m", msg, "--no-commit"], text);
        assert!(ok, "{out}{err}");
    };
    assert!(in_dir(&app, rust, &["init"], "").0);
    edit("main", "start", "fn double(x: Int) -> Int {\n  x * 2\n}\n\nfn a() -> Int {\n  double(1)\n}\n");
    git(&a, &["add", "-A"]);
    git(&a, &["commit", "-qm", "start"]);
    git(&a, &["checkout", "-qb", "feature"]);
    assert!(in_dir(&app, rust, &["rename", "double", "twice", "--no-commit"], "").0);
    git(&a, &["add", "-A"]);
    git(&a, &["commit", "-qm", "rename"]);
    git(&a, &["checkout", "-q", "main"]);
    edit("extra", "extra", "fn c() -> Int {\n  double(7)\n}\n");
    git(&a, &["add", "-A"]);
    git(&a, &["commit", "-qm", "extra"]);
    edit("main", "uncommitted", "fn a() -> Int {\n  double(2)\n}\n");
    let b = scratch("git-b").join("repo");
    copy(&a, &b);
    let same = |args: &[&str]| both(&a, &b, args, "", None);
    for args in [&["diff", "--file", "app"][..], &["diff", "--file", "app", "HEAD~1"], &["diff", "--file", "app", "--base", "HEAD~1", "--step", "1"], &["diff", "--file", "app", "nope"], &["upgrade"], &["sync", "feature"]] {
        same(args);
    }
    // (the uncommitted change refused the sync: dropped)
    for d in [&a, &b] {
        git(d, &["checkout", "-q", "--", "."]);
    }
    for args in [&["sync", "feature"][..], &["sync", "feature"], &["sync", "nope"]] {
        same(args);
    }
    // a conflict: both sides change `a`; left as text, fixed, then stored again
    for d in [&a, &b] {
        let app = d.join("app");
        git(d, &["checkout", "-qb", "other", "feature"]);
        assert!(in_dir(&app, rust, &["edit", "main", "-m", "a 5", "--no-commit"], "fn a() -> Int {\n  twice(5)\n}\n").0);
        git(d, &["commit", "-qam", "a 5"]);
        git(d, &["checkout", "-q", "main"]);
        assert!(in_dir(&app, rust, &["edit", "main", "-m", "a 6", "--no-commit"], "fn a() -> Int {\n  twice(6)\n}\n").0);
        git(d, &["commit", "-qam", "a 6"]);
    }
    same(&["sync", "other"]);
    for d in [&a, &b] {
        std::fs::write(d.join("app/main.strata"), "fn twice(x: Int) -> Int {\n  x * 2\n}\n\nfn a() -> Int {\n  twice(11)\n}\n").unwrap();
    }
    same(&["resolve", "app/main.strata"]);
    same(&["resolve", "app/main.strata"]);
}
