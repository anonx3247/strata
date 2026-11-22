//! `std.ast.resolve` stores a program as `src/resolve.rs`'s `stored_form`
//! does, every reference to a definition qualified, with std in scope: on
//! the scripts of the repository (their text as `strata render` writes it)
//! and on hand-written ones (imports, shadowing locals, module paths).
//! Both sides print each definition as the `std.ast` value `debug` prints
//! (`strata parse FILE --stored`).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const STRATA: &str = env!("CARGO_BIN_EXE_strata");

/// The checker: each file's tokens and comments, or its error, after a
/// line `=== FILE`.
const CHECKER: &str = r#"use std.fs.{Files, Disk}
use std.process.{Environment, Process}
use std.ast.binary.{decode_module}
use std.ast.parse.{parse_program}
use std.ast.resolve.{stored}

fn main(disk: Disk, process: Process) {
  mut std := []
  mut after := false
  for a in process.args() {
    if a == "--" {
      after = true
    } else if !after {
      match disk.read_bytes(a) {
        Ok(bs) => match decode_module(bs) {
          Ok(ds) => {
            std = std + ds
          },
          Err(_) => {},
        },
        Err(_) => {},
      }
    } else {
      print("=== " + a)
      match disk.read(a) {
        Ok(text) => match parse_program(text) {
          Ok(ds) => {
            for d in stored(ds, std) {
              print(debug(d))
            }
          },
          Err(e) => print(f"error: {e}"),
        },
        Err(_) => print("unreadable"),
      }
    }
  }
}
"#;

/// Text rendering never makes, each with what it tests.
const CASES: &[(&str, &str)] = &[
    ("imports", "use std.io.{Console, recorder}\nuse std.fs.*\nuse std.json\nfn f(mut c: Console, x: Int) -> Int { r := recorder([]); mut c.print(\"a\"); y := std.json.parse(\"1\"); Some(x) ?? 0 }\n"),
    ("locals", "type Line = { sku: String }\nfn Line2(x: Int) -> Int { x }\nfn f(Line: Int, print: Int) -> Int { g := (f: Int) -> f + Line; match Some(1) { Some(f) => f, None => print } }\nfn main() { print(f(1, 2)) }\n"),
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
    let checker = dir.join("check.strata");
    let mut c = Command::new(STRATA).args(["--file", &checker.to_string_lossy(), "edit"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(CHECKER.as_bytes()).unwrap();
    assert!(c.wait_with_output().unwrap().status.success());
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
    let mut args = vec!["--file", checker.to_str().unwrap(), "run", "--"];
    args.extend(std_files.iter().map(|s| s.as_str()));
    args.push("--");
    args.extend(paths.iter().map(|s| s.as_str()));
    let got = run(&args);
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
            wrong.push(format!("{name}, definition {}:\n  std.ast.resolve: {}\n  Rust:            {}", at + 1, g.get(at).unwrap_or(&"(none)"), w.get(at).unwrap_or(&"(none)")));
        }
    }
    assert!(wrong.is_empty(), "{} of {} texts store differently:\n{}", wrong.len(), texts.len(), wrong.join("\n"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The checker for packages: std's module files, `--`, then `TEXT=MODULE`
/// pairs; the package's definitions stored together, one per line.
const PACKAGE_CHECKER: &str = r#"use std.fs.{Files, Disk}
use std.process.{Environment, Process}
use std.ast.binary.{decode_module}
use std.ast.parse.{parse_module}
use std.ast.resolve.{stored}

/// Args: std module files, `--`, then `TEXT=MODULE` pairs of one package:
/// the package's definitions in stored form, one per line.
fn main(disk: Disk, process: Process) {
  mut std := []
  mut mine := []
  mut after := false
  for a in process.args() {
    if a == "--" {
      after = true
    } else if !after {
      match disk.read_bytes(a) {
        Ok(bs) => match decode_module(bs) {
          Ok(ds) => {
            std = std + ds
          },
          Err(_) => {},
        },
        Err(_) => {},
      }
    } else {
      parts := split(a, "=")
      file := get(parts, 0) ?? ""
      module := get(parts, 1) ?? ""
      match disk.read(file) {
        Ok(text) => match parse_module(text, module) {
          Ok(ds) => {
            mine = mine + ds
          },
          Err(e) => print(f"error: {e}"),
        },
        Err(_) => print("unreadable"),
      }
    }
  }
  for d in stored(mine, std) {
    print(debug(d))
  }
}
"#;

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

/// Every package of the examples, and std: each module's text (as
/// `render` writes it) parsed as that module, all stored together, are
/// the package's files' trees (`header --tree`: the files are the stored form).
#[test]
fn packages_store_as_in_rust() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("packages");
    let checker = dir.join("check.strata");
    let mut c = Command::new(STRATA).args(["--file", &checker.to_string_lossy(), "edit"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(PACKAGE_CHECKER.as_bytes()).unwrap();
    assert!(c.wait_with_output().unwrap().status.success());
    let mut packages: Vec<(PathBuf, String)> = vec![(root.join("std"), "std.".into())];
    for e in std::fs::read_dir(root.join("examples")).unwrap() {
        let p = e.unwrap().path();
        if p.join("strata.toml").exists() {
            packages.push((p, String::new()));
        }
    }
    let std_files: Vec<String> = std_files().iter().map(|p| p.to_string_lossy().into_owned()).collect();
    assert!(packages.len() > 5);
    for (k, (pkg, prefix)) in packages.iter().enumerate() {
        let mut args: Vec<String> = vec!["--file".into(), checker.to_string_lossy().into(), "run".into(), "--".into()];
        if prefix.is_empty() {
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
        let mut got: Vec<String> = run(&args.iter().map(|s| s.as_str()).collect::<Vec<_>>()).lines().map(String::from).collect();
        got.sort();
        want.sort();
        let wrong: Vec<&String> = got.iter().filter(|g| !want.contains(g)).collect();
        assert!(got.len() == want.len() && wrong.is_empty(), "{}: {} of {} definitions differ, e.g. {:?}", pkg.display(), wrong.len(), want.len(), wrong.first().map(|s| &s[..s.len().min(300)]));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The written-form checker: std's module files, `--`, then one stored
/// module file; its text as `render` writes it.
const WRITTEN_CHECKER: &str = r#"use std.fs.{Files, Disk}
use std.process.{Environment, Process}
use std.ast.binary.{decode_module}
use std.ast.resolve.{written}
use std.ast.text.{module_text}

/// Args: std module files, `--`, then one stored module file: its text as
/// `render` writes it.
fn main(disk: Disk, process: Process) {
  mut std := []
  mut mine := []
  mut after := false
  for a in process.args() {
    if a == "--" {
      after = true
    } else {
      match disk.read_bytes(a) {
        Ok(bs) => match decode_module(bs) {
          Ok(ds) => if after {
            mine = mine + ds
          } else {
            std = std + ds
          },
          Err(e) => print(e),
        },
        Err(_) => print("unreadable"),
      }
    }
  }
  print(module_text(written(mine, std)))
}
"#;

/// Every script of the repository and every module of std, as stored,
/// written back (`std.ast.resolve.written`) and printed (`std.ast.text`),
/// is the text `strata render` shows.
#[test]
fn written_as_rendered() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("written");
    let checker = dir.join("check.strata");
    let mut c = Command::new(STRATA).args(["--file", &checker.to_string_lossy(), "edit"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(WRITTEN_CHECKER.as_bytes()).unwrap();
    assert!(c.wait_with_output().unwrap().status.success());
    let exe = dir.join("check");
    run(&["build", &checker.to_string_lossy(), "-o", &exe.to_string_lossy()]);
    let std_files: Vec<PathBuf> = std_files();
    let mut wrong = vec![];
    let mut cases: Vec<(PathBuf, Vec<String>, String)> = vec![];
    for f in module_files() {
        let want = run(&["--file", &f.to_string_lossy(), "render"]);
        cases.push((f, std_files.iter().map(|p| p.to_string_lossy().into_owned()).collect(), want));
    }
    for (f, m) in package_modules(&root.join("std"), "std.") {
        let want = run(&["--file", &root.join("std").to_string_lossy(), "render", &m]);
        cases.push((f.clone(), std_files.iter().filter(|p| **p != f).map(|p| p.to_string_lossy().into_owned()).collect(), want));
    }
    for (f, others, want) in &cases {
        let o = Command::new(&exe).args(others).arg("--").arg(f).output().unwrap();
        let got = String::from_utf8_lossy(&o.stdout).to_string();
        // (`print` ends the text with one more newline)
        if got != format!("{want}\n") {
            wrong.push(f.display().to_string());
        }
    }
    assert!(wrong.is_empty(), "{} of {} files are written differently: {wrong:?}", wrong.len(), cases.len());
    let _ = std::fs::remove_dir_all(&dir);
}
