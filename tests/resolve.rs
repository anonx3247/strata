//! `compiler/resolve` (the compiler package's `stored` commands) stores a
//! program as `src/resolve.rs`'s `stored_form` does, every reference to a
//! definition qualified, with std in scope: on the scripts of the
//! repository (their text as `strata render` writes it) and on hand-written
//! ones (imports, shadowing locals, module paths).
//! Both sides print each definition as the `ast` value `debug` prints
//! (`strata parse FILE --stored`).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[path = "common/compiler.rs"]
mod compiler;

const STRATA: &str = env!("CARGO_BIN_EXE_strata");

/// Text rendering never makes, each with what it tests.
const CASES: &[(&str, &str)] = &[
    ("imports", "from std.io use Console, recorder as rec\nuse std.fs as files\nuse std.json\nfn f(mut c: Console, x: Int, d: files.Disk) -> Int { r := rec([]); mut c.print(\"a\"); y := json.parse(\"1\"); z := std.json.parse(\"2\"); Some(x) ?? 0 }\n"),
    ("locals", "type Line = { sku: String }\nfn Line2(x: Int) -> Int { x }\nfn f(Line: Int, print: Int) -> Int { g := (f: Int) -> f + Line; match Some(1) { Some(f) => f, None => print } }\nfn main() { print(f(1, 2)) }\n"),
    ("unions", "type Circle = { r: Float }\ntype Square = { s: Float }\ntype Shape = | Circle | Square\ntype Mixed = | Circle | Dot\ntype P = | Some(Int) | Other\ntype G[T] = | T | Leaf\nfn f(s: Shape) -> Int { 0 }\n"),
    ("types", "type T[A] = | Leaf | Node(T[A], A, T[A])\ntrait Size { fn size(self) -> Int }\nimpl[A] Size for T[A] { fn size(self) -> Int { match self { Leaf => 0, Node(l, _, r) => l.size() + 1 + r.size() } } }\nfn g[A: Size](x: A) -> Int where it > 0 { x.size() }\n"),
];

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("strata-resolve-test-{name}-{}", std::process::id()));
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
    // scripts only (a package's modules are stored together: not yet)
    let mut todo = vec![root.join("examples"), root.join("tests/data")];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n == "build") {
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

/// std's module files (what a program may `use`).
fn std_files() -> Vec<PathBuf> {
    let mut out = vec![];
    let mut todo = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("std")];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                if !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
                    todo.push(p);
                }
            } else if p.extension().is_some_and(|x| x == "strata") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn stores_as_in_rust() {
    let dir = scratch("all");
    // the text of each module file, as rendered, and the cases
    let mut texts = vec![];
    for (i, f) in module_files().iter().enumerate() {
        let text = run(&["--file", &f.to_string_lossy(), "render"]);
        let p = dir.join(format!("module{i}.st"));
        std::fs::write(&p, text).unwrap();
        texts.push((f.display().to_string(), p));
    }
    assert!(texts.len() > 10);
    for (name, text) in CASES {
        let p = dir.join(format!("{name}.st"));
        std::fs::write(&p, text).unwrap();
        texts.push((name.to_string(), p));
    }
    let paths: Vec<String> = texts.iter().map(|t| t.1.to_string_lossy().into_owned()).collect();
    let std_files: Vec<String> = std_files().iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let mut args = vec!["stored"];
    args.extend(std_files.iter().map(|s| s.as_str()));
    args.push("--");
    args.extend(paths.iter().map(|s| s.as_str()));
    let got = compiler::run(&args);
    let mut wrong = vec![];
    for (i, (name, p)) in texts.iter().enumerate() {
        let start = format!("=== {}\n", p.display());
        let from = got.find(&start).unwrap_or_else(|| panic!("no trees for {name}")) + start.len();
        let until = paths.get(i + 1).and_then(|n| got[from..].find(&format!("=== {n}\n")).map(|k| from + k)).unwrap_or(got.len());
        let want = run(&["parse", &p.to_string_lossy(), "--stored"]);
        let strata = &got[from..until];
        if strata != want {
            let (g, w): (Vec<&str>, Vec<&str>) = (strata.lines().collect(), want.lines().collect());
            let at = g.iter().zip(&w).position(|(a, b)| a != b).unwrap_or(g.len().min(w.len()));
            wrong.push(format!("{name}, definition {}:\n  compiler (resolve): {}\n  Rust:               {}", at + 1, g.get(at).unwrap_or(&"(none)"), w.get(at).unwrap_or(&"(none)")));
        }
    }
    assert!(wrong.is_empty(), "{} of {} texts store differently:\n{}", wrong.len(), texts.len(), wrong.join("\n"));
    let _ = std::fs::remove_dir_all(&dir);
}


/// A package's modules (a folder's `.strata` files), each named after its
/// path (`main` is the root), with a prefix (`std.` for std).
fn package_modules(dir: &Path, prefix: &str) -> Vec<(PathBuf, String)> {
    let mut out = vec![];
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                if !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
                    todo.push(p);
                }
            } else if p.extension().is_some_and(|x| x == "strata") {
                let rel = p.strip_prefix(dir).unwrap().with_extension("").to_string_lossy().replace('/', ".");
                let m = if rel == "main" && prefix.is_empty() { String::new() } else { format!("{prefix}{rel}") };
                out.push((p, m));
            }
        }
    }
    out.sort();
    out
}

/// Every package of the examples, std and the compiler: each module's text (as
/// `render` writes it) parsed as that module, all stored together, are
/// the package's files' trees (`header --tree`: the files are the stored form).
#[test]
fn packages_store_as_in_rust() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("packages");
    let mut packages: Vec<(PathBuf, String)> = vec![(root.join("std"), "std.".into()), (root.join("compiler"), String::new())];
    for e in std::fs::read_dir(root.join("examples")).unwrap() {
        let p = e.unwrap().path();
        if p.join("strata.toml").exists() {
            packages.push((p, String::new()));
        }
    }
    let std_files: Vec<String> = std_files().iter().map(|p| p.to_string_lossy().into_owned()).collect();
    assert!(packages.len() > 5);
    for (k, (pkg, prefix)) in packages.iter().enumerate() {
        let mut args: Vec<String> = vec!["stored-package".into()];
        if prefix != "std." {
            args.extend(std_files.iter().cloned());
        }
        args.push("--".into());
        let mut want: Vec<String> = vec![];
        for (i, (file, m)) in package_modules(pkg, prefix).iter().enumerate() {
            let text = run(&["--file", &pkg.to_string_lossy(), "render", if m.is_empty() { "main" } else { m }]);
            let t = dir.join(format!("p{k}m{i}.st"));
            std::fs::write(&t, text).unwrap();
            args.push(format!("{}={m}", t.display()));
            want.extend(run(&["header", &file.to_string_lossy(), "--tree"]).lines().map(String::from));
        }
        let mut got: Vec<String> = compiler::run(&args).lines().map(String::from).collect();
        got.sort();
        want.sort();
        let wrong: Vec<&String> = got.iter().filter(|g| !want.contains(g)).collect();
        assert!(got.len() == want.len() && wrong.is_empty(), "{}: {} of {} definitions differ, e.g. {:?}", pkg.display(), wrong.len(), want.len(), wrong.first().map(|s| &s[..s.len().min(300)]));
    }
    let _ = std::fs::remove_dir_all(&dir);
}


/// Every script of the repository and every module of std and of the
/// compiler, as stored, written back (`compiler/resolve`, the
/// compiler's `written` command) and printed (`compiler/text`),
/// is the text `strata render` shows.
#[test]
fn written_as_rendered() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("written");
    let std_files: Vec<PathBuf> = std_files();
    let mut wrong = vec![];
    let mut cases: Vec<(PathBuf, Vec<String>, String)> = vec![];
    for f in module_files() {
        let want = run(&["--file", &f.to_string_lossy(), "render"]);
        cases.push((f, std_files.iter().map(|p| p.to_string_lossy().into_owned()).collect(), want));
    }
    // unions written as the named ones (`named_unions`), which the
    // repository's code doesn't need
    let unions = dir.join("unions.strata");
    let mut c = Command::new(STRATA).args(["--file", &unions.to_string_lossy(), "edit"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(b"/// a circle\ntype Circle = { r: Float }\n/// a square\ntype Square = { s: Float }\ntype Dot = { at: Int }\ntype Shape = Circle | Square\ntype Pair[A, B] = List[A] | Option[B]\nfn area(s: Circle | Square) -> Float { 0.0 }\nfn any(s: Square | Circle | Dot) -> Int { 0 }\nfn pair(p: List[Int] | Option[String]) -> Int { 0 }\n").unwrap();
    assert!(c.wait_with_output().unwrap().status.success());
    let rendered = run(&["--file", &unions.to_string_lossy(), "render"]);
    assert!(rendered.contains("fn area(s: Shape)") && rendered.contains("Shape | Dot") && rendered.contains("Pair[Int, String]"), "{rendered}");
    cases.push((unions, std_files.iter().map(|p| p.to_string_lossy().into_owned()).collect(), rendered));
    // and, without docs, the members shown inside their union (`members_view`)
    let inline = dir.join("inline.strata");
    let mut c = Command::new(STRATA).args(["--file", &inline.to_string_lossy(), "edit"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(b"type Circle = { r: Float }\ntype Dot\ntype Square = { s: Float }\nimpl Square { fn area(self) -> Float { 0.0 } }\ntype Shape = Circle | Dot | Square\n").unwrap();
    assert!(c.wait_with_output().unwrap().status.success());
    let rendered = run(&["--file", &inline.to_string_lossy(), "render"]);
    assert!(rendered.contains("| Circle { r: Float }"), "{rendered}");
    cases.push((inline, std_files.iter().map(|p| p.to_string_lossy().into_owned()).collect(), rendered));
    for (f, m) in package_modules(&root.join("std"), "std.") {
        let want = run(&["--file", &root.join("std").to_string_lossy(), "render", &m]);
        cases.push((f.clone(), std_files.iter().filter(|p| **p != f).map(|p| p.to_string_lossy().into_owned()).collect(), want));
    }
    // and every module of the compiler, with std's and its other modules
    let compiler_modules = package_modules(&root.join("compiler"), "");
    for (f, m) in &compiler_modules {
        let want = run(&["--file", &root.join("compiler").to_string_lossy(), "render", if m.is_empty() { "main" } else { m }]);
        let others = std_files.iter().chain(compiler_modules.iter().map(|c| &c.0)).filter(|p| *p != f).map(|p| p.to_string_lossy().into_owned()).collect();
        cases.push((f.clone(), others, want));
    }
    for (f, others, want) in &cases {
        let o = Command::new(compiler::exe()).arg("written").args(others).arg("--").arg(f).output().unwrap();
        let got = String::from_utf8_lossy(&o.stdout).to_string();
        // (`print` ends the text with one more newline)
        if got != format!("{want}\n") {
            wrong.push(f.display().to_string());
        }
    }
    assert!(wrong.is_empty(), "{} of {} files are written differently: {wrong:?}", wrong.len(), cases.len());
    let _ = std::fs::remove_dir_all(&dir);
}


/// A `use` of a module there isn't, of what a module doesn't have, or of a
/// private definition: the errors `strata check` gives for them.
#[test]
fn use_errors_as_in_rust() {
    let dir = scratch("errors");
    let pkg = dir.join("pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("strata.toml"), "name = \"p\"\n").unwrap();
    for (m, text) in [("util", "pub fn shown(x: Int) -> Int { x }\nfn secret() -> Int { 1 }\npub mock Fake = {}\npub extern c fn strlen(s: String) -> U64\n"), ("main", "from nothere use x\nfrom util use shown, missing, secret, Fake\nfn main() { print(shown(1) + util.secret()) f := Fake {} n := util.strlen(\"a\") }\nfn g() -> Int { j := std.json.parse(\"1\") 0 }\ntest \"fake\" { f := Fake {} }\n")] {
        let mut c = Command::new(STRATA).args(["--file", &format!("{}/", pkg.display()), "edit", m]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        c.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
        c.wait_with_output().unwrap();
    }
    let mut args: Vec<String> = vec!["use-errors".into()];
    args.extend(std_files().iter().map(|p| p.to_string_lossy().into_owned()));
    args.push("--".into());
    args.extend(["util.strata", "main.strata"].iter().map(|f| pkg.join(f).to_string_lossy().into_owned()));
    let got: Vec<String> = compiler::run(&args).lines().map(String::from).collect();
    let o = Command::new(STRATA).args(["--file", &format!("{}/", pkg.display()), "check"]).output().unwrap();
    let check = String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr);
    let want: Vec<String> = check.lines().filter(|l| l.starts_with("error in use ") || l.contains("is private to") || l.contains("internal to the standard library") || l.contains("is a mock") || l.contains("foreign function") || l.contains("which this module doesn't use")).map(String::from).collect();
    assert!(got.len() >= 5, "{got:?}");
    assert_eq!(got, want, "{check}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Write `text` as module `m` of the package in `dir` (named `name`, with
/// `deps` as its `[deps]` lines).
fn package_module(dir: &Path, name: &str, deps: &str, m: &str, text: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("strata.toml"), format!("name = \"{name}\"\n[deps]\n{deps}")).unwrap();
    if text.is_empty() {
        return;
    }
    let mut c = Command::new(STRATA).args(["--file", &format!("{}/", dir.display()), "edit", m]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    c.wait_with_output().unwrap();
}

/// Path dependencies (`compiler/deps`, the compiler's `package` command): a
/// dependency's modules are named with its package's name (`json`,
/// `json.text`), reached through `use`; its private definitions are refused;
/// a missing dependency and a cycle between packages are errors.
#[test]
fn path_dependencies() {
    let dir = scratch("deps");
    let std_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("std");
    let package = |p: &str| compiler::run(&["package", "--std", &std_dir.to_string_lossy(), &dir.join(p).to_string_lossy()]);
    let json = dir.join("json");
    package_module(&json, "json", "", "main", "pub type Tok = { n: Int }\npub fn parse(s: String) -> Tok { Tok { n: helper(s) } }\nfn helper(s: String) -> Int { 1 }\n");
    package_module(&json, "json", "", "text", "pub fn twice(s: String) -> Int { parse(s).n + helper(s) }\n");
    let dep = "json = { path = \"../json\" }\n";
    package_module(&dir.join("app"), "app", dep, "main", "from json use parse, Tok\nuse json.text\nfn first(t: Tok) -> Int { t.n }\nfn main() { print(first(parse(\"x\")) + text.twice(\"y\")) }\n");
    let out = package("app");
    assert!(!out.contains("error"), "{out}");
    assert!(out.contains("fn first(t: json.Tok)") && out.contains("json.parse(\"x\")") && out.contains("json.text.twice(\"y\")"), "{out}");
    package_module(&dir.join("private"), "private", dep, "main", "from json use helper\nfn main() { print(helper(\"x\")) }\n");
    assert!(package("private").contains("error in main: `helper` is private to module `json`"));
    package_module(&dir.join("missing"), "missing", "nope = { path = \"../nope\" }\n", "", "");
    let out = package("missing");
    assert!(out.starts_with("error: dependency `nope`: no package at"), "{out}");
    package_module(&dir.join("a"), "a", "b = { path = \"../b\" }\n", "", "");
    package_module(&dir.join("b"), "b", "c = { path = \"../c\" }\n", "", "");
    package_module(&dir.join("c"), "c", "a = { path = \"../a\" }\n", "", "");
    assert_eq!(package("a").trim(), "error: dependency cycle: a -> b -> c -> a");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Git dependencies (`compiler/deps`): `[deps]` names a git URL (here a
/// local bare repository, `file://`), fetched into the cache
/// (`$STRATA_CACHE`) and pinned in strata.lock, which later runs reuse until
/// `update` moves it.
#[test]
fn git_dependencies() {
    let dir = scratch("git-deps");
    let std_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("std");
    let cache = dir.join("cache");
    let run = |args: &[&str]| {
        let o = Command::new(compiler::exe()).args(args).env("STRATA_CACHE", &cache).output().unwrap();
        String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr)
    };
    let git = |d: &Path, args: &[&str]| {
        let o = Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"]).args(args).current_dir(d).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    };
    // the dependency's repository: a commit tagged v1, then a second one
    let src = dir.join("json");
    package_module(&src, "json", "", "main", "pub fn parse(s: String) -> Int { 1 }\n");
    git(&src, &["init", "-q"]);
    git(&src, &["add", "."]);
    git(&src, &["commit", "-q", "-m", "one"]);
    git(&src, &["tag", "v1"]);
    let first = git(&src, &["rev-parse", "HEAD"]);
    git(&dir, &["clone", "-q", "--bare", "json", "json.git"]);
    let url = format!("file://{}", dir.join("json.git").display());
    let app = dir.join("app");
    package_module(&app, "app", &format!("json = \"{url}\"\n"), "main", "from json use parse\nfn main() { print(parse(\"x\")) }\n");
    let package = || run(&["package", "--std", &std_dir.to_string_lossy(), &app.to_string_lossy()]);
    let out = package();
    assert!(!out.contains("error") && out.contains("json.parse(\"x\")"), "{out}");
    let lock = std::fs::read_to_string(app.join("strata.lock")).unwrap();
    assert!(lock.contains(&format!("[[git]]\nname = \"json\"\nurl = \"{url}\"\ncommit = \"{first}\"\n")), "{lock}");
    // the cache: a bare clone, and the commit's files
    let key = std::fs::read_dir(cache.join("deps/repos")).unwrap().next().unwrap().unwrap().file_name().to_string_lossy().trim_end_matches(".git").to_string();
    assert!(key.starts_with(&format!("{}-", dir.join("json.git").display().to_string().replace('/', "_"))) && key.len() > 9, "{key}");
    assert!(cache.join("deps").join(&key).join(&first).join("main.strata").exists());
    // a new commit upstream: the lock is kept (not even rewritten) until `update`
    std::fs::write(src.join("notes.txt"), "two").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-q", "-m", "two"]);
    let second = git(&src, &["rev-parse", "HEAD"]);
    git(&src, &["push", "-q", &dir.join("json.git").to_string_lossy(), "main"]);
    let before = std::fs::metadata(app.join("strata.lock")).unwrap().modified().unwrap();
    assert!(!package().contains("error"));
    assert_eq!(std::fs::read_to_string(app.join("strata.lock")).unwrap(), lock);
    assert_eq!(std::fs::metadata(app.join("strata.lock")).unwrap().modified().unwrap(), before);
    let out = run(&["update", &app.to_string_lossy()]);
    assert!(out.contains(&format!("commit = \"{second}\"")), "{out}");
    assert!(cache.join("deps").join(&key).join(&second).join("notes.txt").exists());
    // a tag, and a branch of the same repository: two entries, sorted
    package_module(&app, "app", &format!("json = {{ git = \"{url}\", tag = \"v1\" }}\n"), "", "");
    assert!(!package().contains("error"));
    let lock = std::fs::read_to_string(app.join("strata.lock")).unwrap();
    assert!(lock.contains(&format!("tag = \"v1\"\ncommit = \"{first}\"")) && !lock.contains(&second), "{lock}");
    // what can't be: a missing tag, two of rev/tag/branch, unknown keys
    package_module(&app, "app", &format!("json = {{ git = \"{url}\", tag = \"v9\" }}\n"), "", "");
    assert!(package().contains(&format!("error: dependency `json`: {url} has no tag v9")));
    package_module(&app, "app", &format!("json = {{ git = \"{url}\", tag = \"v1\", branch = \"main\" }}\n"), "", "");
    assert!(package().contains("at most one of rev, tag and branch"));
    package_module(&app, "app", "json = { git = \"x\", path = \"../json\" }\n", "", "");
    assert!(package().contains("`path` or `git`, not both"));
    package_module(&app, "app", "json = { got = \"x\" }\n", "", "");
    assert!(package().contains("unknown key `got`"));
    let _ = std::fs::remove_dir_all(&dir);
}
