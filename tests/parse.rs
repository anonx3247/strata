//! `compiler/parse` (the compiler package's `parse` command) parses text
//! into the trees `src/parser.rs` makes, comments included, and refuses what it refuses with the same message: on
//! the text of every module file in the repository (as `strata render`
//! writes it) and on hand-written text with what rendering never produces
//! (comments in odd places, shorthands, every kind of error). Both sides
//! print each definition as the `ast` value `debug` prints
//! (`strata parse`).

use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "common/compiler.rs"]
mod compiler;

const STRATA: &str = env!("CARGO_BIN_EXE_strata");

/// Text rendering never makes, each with what it tests.
const CASES: &[(&str, &str)] = &[
    ("comments", "// a\n\n// b\n/// doc\n// between\npub fn f(x: Int) -> Int { // trailing\n  // above\n  y := x\n\n  // after a blank\n  y + 1 // end\n} // after\n\ntype T = {\n  // c\n  /// d\n  a: Int, // t\n  b: Int = 2,\n}\n\ntype S =\n  // v\n  | A // w\n  | B(Int)\n  /// A doc.\n  ///\n  /// Two paragraphs.\n  | C { x: Int }\n\n/// The next one's.\nfn n() {}\n"),
    ("shorthands", "from a.b use c, d as e\nuse a.b as ab\nuse a.b\nuse a\nextern c fn strlen(s: String) -> U64\nextern c type Handle\nfn g[T: Ord + Hash, U](mut x: T, comptime y: U) -> T requires x > 0 ensures it > 0 terminates = ?todo\nfn h() { q := [x * 2 for x in xs where x > 0 for y in ys]; s := Set { x for x in xs }; d := Dict { k: v for (k, v) in kv }; e := Dict[String, Int] {}; f := (a, b) -> a + b; g := x -> x; m := match v as w { Some(x) => x, None => 0, }; r := xs[1..]; r2 := xs[..=2]; r3 := xs[a..b]; mut q.claim(2).len(); z := L { ..l, a: 1, b }; t := (1, \"a\"); u := -3 + -x; v2 := !!b; w2 := a ?? b !! c; return }\n"),
    ("tasks", "fn t(rx: Receiver[Int], tx: Sender[Int]) { scope { h := spawn f(1) + 2; loop { select { // c\n rx.recv() as x => g(x), tx.send(1) => (), await h as r => break, } } }; w := race { // r\n f(1) as a => a, { g(2) } => 0, }; select(a, b); race(c); s := supervised spawn f(1) + 2; v := await s except p { 0 }; u := await s!.len(); t := await (s!); supervised := 3; q := supervised\n spawn g(supervised); k := race { supervised f(1) as r => r, supervised { g(2) } => 0, supervised as y => y, } }\n"),
    ("patterns", "fn p(v: Int) -> Int { match v { 0 | 1 => 1, -2 => 2, where it > 3 => 3, \"s\" => 4, [a, b, ..rest] => 5, [..] => 6, (x, _) => 7, Some(Circle { r, .. }) => 8, shop.Pending { id: i } => 9, Option[x] => 10, true => 11, _ => 0 } }\n"),
    ("variant_docs", "/// The type.\ntype C =\n  /// First.\n  | A\n  // a comment\n  /// Second.\n  | B(Int)\n\n/// Not a member.\nfn f() {}\n"),
    ("let_untyped", "fn f() { let x = 1 }"),
    ("assign_call", "fn f() { g(x) = 1 }"),
    ("mut_call", "fn f() { mut g(x) }"),
    ("decl_keyword", "fn f() { 1 }\nwhile x {}"),
    ("plus_plus", "fn f() { a ++ b }"),
    ("lost_comment", "fn f() { g(1, // x\n 2) }"),
    ("fstring_spec", "fn f() { a := f\"{x:bad}\" }"),
    ("fstring_comment", "fn f() { a := f\"{x // c\n}\" }"),
    ("effect", "effect E {}"),
    ("big_pattern", "fn f(x: Int) -> Int { match x { 99999999999999999999 => 1, _ => 0 } }"),
    ("no_body", "fn f() -> Int"),
    ("lambda_bars", "fn f() { |x| x }"),
    ("unclosed", "fn f( {"),
];

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("strata-parse-test-{name}-{}", std::process::id()));
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
fn parses_as_in_rust() {
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
    let mut args = vec!["parse"];
    args.extend(paths.iter().map(|s| s.as_str()));
    let got = compiler::run(&args);
    let mut wrong = vec![];
    for (i, (name, p)) in texts.iter().enumerate() {
        let start = format!("=== {}\n", p.display());
        let from = got.find(&start).unwrap_or_else(|| panic!("no trees for {name}")) + start.len();
        let until = paths.get(i + 1).and_then(|n| got[from..].find(&format!("=== {n}\n")).map(|k| from + k)).unwrap_or(got.len());
        let want = run(&["parse", "--rust", &p.to_string_lossy()]);
        let strata = &got[from..until];
        if strata != want {
            let (g, w): (Vec<&str>, Vec<&str>) = (strata.lines().collect(), want.lines().collect());
            let at = g.iter().zip(&w).position(|(a, b)| a != b).unwrap_or(g.len().min(w.len()));
            wrong.push(format!("{name}, definition {}:\n  compiler/parse: {}\n  Rust:           {}", at + 1, g.get(at).unwrap_or(&"(none)"), w.get(at).unwrap_or(&"(none)")));
        }
    }
    assert!(wrong.is_empty(), "{} of {} texts parse differently:\n{}", wrong.len(), texts.len(), wrong.join("\n"));
    let _ = std::fs::remove_dir_all(&dir);
}
