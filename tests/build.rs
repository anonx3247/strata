//! `strata build`: a compiled program prints what the interpreter prints,
//! faults included, and exits with the same code.

use std::path::{Path, PathBuf};
use std::io::Write;
use std::process::{Command, Stdio};

#[path = "common/programs.rs"]
mod programs;
use programs::{tests_as_main, BIG_INTEGERS, MEMORY, NESTED_ASSIGN, TRAIT_METHODS, COLLECTIONS, DECLARED_UNIONS, FIELDS, STRINGS, TRAIT_EQUALITY, UNIONS, UNION_METHODS};


/// Store a program written as text at `path` (a module file): text only goes
/// in through an edit.
fn save(path: &std::path::Path, src: impl AsRef<str>) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_strata"))
        .args(["--file", &path.to_string_lossy(), "edit"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(src.as_ref().as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success(), "{}: {}", path.display(), String::from_utf8_lossy(&o.stderr));
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("strata-build-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn have_clang() -> bool {
    let cc = std::env::var("STRATA_CC").unwrap_or_else(|_| "clang".into());
    Command::new(cc).arg("--version").output().is_ok_and(|o| o.status.success())
}

/// Output (stdout then stderr) and exit code.
fn output(c: &mut Command) -> (String, i32) {
    let o = c.output().unwrap();
    // the interpreter also prints the call stack of a fault
    let err: String = String::from_utf8_lossy(&o.stderr).lines().filter(|l| !l.starts_with("    stack:")).map(|l| format!("{l}\n")).collect();
    (String::from_utf8_lossy(&o.stdout).to_string() + &err, o.status.code().unwrap_or(-1))
}

/// The arguments every program is run with.
const ARGS: &[&str] = &["one", "two words"];

/// A run's output and exit code against the one recorded as `key` in
/// tests/data/build/expected (recorded from the interpreter's runs, which
/// compiled code was compared with until the interpreter's retirement:
/// IDEAS.md). `STRATA_BLESS=1` records it instead.
fn expected(key: &str, got: &(String, i32)) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/build/expected").join(format!("{key}.out"));
    let text = format!("{}\n--- exit {}\n", got.0, got.1);
    if std::env::var_os("STRATA_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("no expected output {} (STRATA_BLESS=1 records it)", path.display()));
    assert_eq!(text, want, "{key}: the output (left) differs from the expected one (right), {}", path.display());
}

/// Build `file` natively and run it: the expected output (`expected`, keyed
/// by the test's folder and the file's name), everything freed.
fn built_as_expected(file: &Path, dir: &Path) -> (String, i32) {
    let strata = env!("CARGO_BIN_EXE_strata");
    let exe = dir.join("prog");
    let b = Command::new(strata).arg("build").arg(file).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    // run in `dir`, with the arguments (what it reads and writes stays there)
    let native = freed_everything(output(Command::new(&exe).args(ARGS).current_dir(dir).env("STRATA_MEMORY", "check")), file);
    // the folder is `strata-build-test-{name}-{pid}` (`tmp`)
    let folder = dir.file_name().unwrap().to_string_lossy().to_string();
    let name = folder.trim_start_matches("strata-build-test-").rsplit_once('-').map_or(folder.as_str(), |(n, _)| n).to_string();
    let stem = file.file_stem().unwrap().to_string_lossy();
    let key = if file.starts_with(dir) { format!("{name}-{stem}") } else { stem.to_string() };
    expected(&key, &native);
    native
}

/// A compiled program's output without its memory report (`STRATA_MEMORY`),
/// which must say it freed everything it made, unless a fault stopped it.
fn freed_everything((out, code): (String, i32), file: &Path) -> (String, i32) {
    // the report, then the objects still alive (in check mode)
    let report = out.lines().find(|l| l.starts_with("memory: ")).unwrap_or("").to_string();
    let rest: String = out.lines().filter(|l| !l.starts_with("memory: ") && !l.starts_with("  live: ")).map(|l| format!("{l}\n")).collect();
    assert!(!report.contains("of a freed object"), "{}: {report}\n{out}", file.display());
    if code != 101 {
        assert!(report.ends_with(" 0 live"), "{}: {report}\n{out}", file.display());
    }
    (rest, code)
}

fn case(dir: &Path, name: &str, src: &str) -> (String, i32) {
    let f = dir.join(format!("{name}.strata"));
    save(&f, src);
    built_as_expected(&f, dir)
}

#[test]
fn checked_float_operations_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("floats");
    let (out, code) = case(&d, "checked", programs::CHECKED_FLOATS);
    assert!(out.starts_with("[Some(3.5), Some(1.0000000000000001e+295), None, None, None, Some(-1.5), Some(0.5), None]\n") && code == 101, "{out}");
}

#[test]
fn numbers_compile_and_run_as_interpreted() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("numbers");
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/build/numbers.strata");
    let (out, code) = built_as_expected(&file, &d);
    assert!(out.starts_with("2432902008176640000\n"), "{out}");
    assert!(out.contains("fault (Refinement): result of `next_digit`"), "{out}");
    assert_eq!(code, 101);
}

#[test]
fn faults_and_control_flow_match_the_interpreter() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("faults");
    let cases = programs::FAULTS;
    for &(name, src, want) in cases {
        let (out, code) = case(&d, name, src);
        assert_eq!(code, want, "{name}: {out}");
    }
}

#[test]
fn records_variants_and_generics_run_as_interpreted() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("data");
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/build/data.strata");
    let (out, code) = built_as_expected(&file, &d);
    assert!(out.contains("Err(\"3 is odd\")") && out.contains("big circle, circle, rect or dot, nothing"), "{out}");
    assert_eq!(code, 101);
    let cases = programs::DATA;
    for &(name, src, want) in cases {
        let (out, code) = case(&d, name, src);
        assert_eq!(code, want, "{name}: {out}");
    }
}

#[test]
fn lists_and_lambdas_run_as_interpreted() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("lists");
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/build/lists.strata");
    let (out, code) = built_as_expected(&file, &d);
    assert!(out.contains("[2, 3, 5, 7, 11, 13, 17, 19, 23, 29]") && out.contains("[0]: -1 does not satisfy `Positive`"), "{out}");
    assert_eq!(code, 101);
    let math = programs::math();
    let (out, code) = case(&d, "math", &math);
    assert!(out.contains("[120, 6, 1024, 10, 9, 55, 4, 8]") && out.contains("division by zero"), "{out}");
    assert_eq!(code, 101);
    // no limit on a range's length (the interpreter's was 1,000,000)
    let (out, code) = case(&d, "range", "fn main() {\n  mut n: Int = 0\n  for i in range(0, 2000000) { n = n + i }\n  print(n)\n  print(len(range(0, 1500000)))\n}");
    assert_eq!((out.as_str(), code), ("1999999000000\n1500000\n", 0));
    // a list or tuple of constants is one constant (never freed); changing it copies it
    let src = "fn table() -> List[(Int, Int)] {\n  [(1, 2), (3, 4)]\n}\n\nfn grow(mut xs: List[String]) {\n  xs = push(xs, \"d\")\n}\n\nfn main() {\n  mut xs := table()\n  xs = push(xs, (5, 6))\n  mut t := (1, 2)\n  t.0 = 9\n  mut zs := [\"a\", \"b\", \"c\"]\n  grow(mut zs)\n  print(f\"{xs} {table()} {t} {(1, 2)} {zs} {[\"a\", \"b\", \"c\"]} {[[1], [2, 3]]} {[None, Some(1)]}\")\n}\n";
    let (out, code) = case(&d, "constants", src);
    assert_eq!((out.as_str(), code), ("[(1, 2), (3, 4), (5, 6)] [(1, 2), (3, 4)] (9, 2) (1, 2) [\"a\", \"b\", \"c\", \"d\"] [\"a\", \"b\", \"c\"] [[1], [2, 3]] [None, Some(1)]\n", 0));
    // constructors of constants too: records with fields of each kind, variants, Some/Ok
    let src = "type P = { name: String, n: Int, x: Float, on: Bool }\ntype Shape =\n  | Dot\n  | Circle(Float)\n  | Named { label: String, sides: Int }\n\nfn main() {\n  p := P { name: \"a\", n: 1, x: 1.5, on: true }\n  mut q := P { name: \"a\", n: 1, x: 1.5, on: true }\n  q.n = 2\n  print(f\"{p} {q} {Some(1)} {[Some(2), None]} {Circle(2.5)} {Named { label: \"sq\", sides: 4 }} {Ok(1) == Ok(1)}\")\n}\n";
    let (out, code) = case(&d, "constant_ctors", src);
    assert_eq!((out.as_str(), code), ("P { name: \"a\", n: 1, x: 1.5, on: true } P { name: \"a\", n: 2, x: 1.5, on: true } Some(1) [Some(2), None] Circle(2.5) Named { label: \"sq\", sides: 4 } true\n", 0));
}

#[test]
fn traits_run_as_interpreted() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("traits");
    // examples/traits.strata, its tests as `main` (without the regex of `Sku`)
    let src = tests_as_main("examples/traits.strata", "");
    let (out, code) = case(&d, "traits_example", &src);
    assert!(out.contains("all asserts hold") && code == 0, "{out}");
    let cases = programs::TRAITS;
    for &(name, src, want) in cases {
        let (out, code) = case(&d, name, src);
        assert_eq!(code, want, "{name}: {out}");
    }
}

#[test]
fn capabilities_and_mut_run_as_interpreted() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("capabilities");
    let cases = programs::CAPABILITIES;
    for &(name, src) in cases {
        let (out, code) = case(&d, name, src);
        assert_eq!(code, 0, "{name}: {out}");
    }
    // the files test made and read a folder of its own, there
    assert!(d.join("fsplay/a.txt").exists());
    let (out, _) = case(&d, "args", programs::ARGS);
    assert!(out.contains(r#"["one", "two words"]"#), "{out}");
}

#[test]
fn strings_dicts_and_sets_run_as_interpreted() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("collections");
    // examples/builtins.strata, its tests as `main`
    let (out, code) = case(&d, "builtins_example", &tests_as_main("examples/builtins.strata", ""));
    assert!(out.contains("all asserts hold") && code == 0, "{out}");
    // examples/wc: its functions, with stand-ins for the console and the files (not its options)
    let wc = Command::new(env!("CARGO_BIN_EXE_strata")).args(["render", "--file"]).arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/wc/main.strata")).output().unwrap();
    let wc = String::from_utf8(wc.stdout).unwrap();
    let wc = wc[..wc.find("test \"").unwrap()].replace("fn main(", "fn wc(") + WC_MAIN;
    let (out, code) = case(&d, "wc", &wc);
    assert!(out.contains(r#"["2\t3\t14\ta.txt", "1\t2\t12\tb.txt", "3\t5\t26\ttotal"]"#) && out.contains("NotFound") && code == 0, "{out}");
    let (out, code) = case(&d, "strings", STRINGS);
    assert!(out.contains(r#"["STRASSE Ǆ ʼN", "οδος σ ας. i̇", "SS", "ǆ"]"#) && out.contains("[-00042]") && code == 0, "{out}");
    let (out, code) = case(&d, "dicts_and_sets", COLLECTIONS);
    assert!(out.contains(r#"Dict { "cat": 2, "end": 1, "hat": 1, "the": 3 }"#) && out.contains("Dict { 1: \"a\", 2: \"b\", 3: \"z\" }") && code == 0, "{out}");
    let faults = programs::COLLECTION_FAULTS;
    for &(name, src) in faults {
        let (out, code) = case(&d, name, src);
        assert!(code == 101 && out.contains("fault ("), "{name}: {out}");
    }
    // `split`, `replace` and `repeat` are total: no argument is wrong
    let (out, code) = case(&d, "total_strings", "fn main() { print(split(\"abc\", \"\")) print(replace(\"abc\", \"\", \"x\")) print(repeat(\"ab\", 3)) print(repeat(\"ab\", -1)) }");
    assert_eq!((out.as_str(), code), ("[\"\", \"a\", \"b\", \"c\", \"\"]\nxaxbxcx\nababab\n\n", 0));
}

const WC_MAIN: &str = r#"fn main() {
  files := mem_files([("a.txt", "one two\nthree\n"), ("b.txt", "four  five" + chr(13) + "\n")])
  for (only, names) in [(None, ["a.txt", "b.txt"]), (Some(Words), ["a.txt"]), (Some(Chars), ["b.txt", "a.txt"]), (None, ["nope.txt"])] {
    mut console := recorder([])
    r := wc(Options { only: only, files: names }, mut console, files)
    print(r)
    print(printed(console))
  }
  mut console := recorder(["hello world", "bye"])
  print(wc(Options { only: None, files: [] }, mut console, mem_files([])))
  print(printed(console))
  print(count("a b  c\n\nd"))
}
"#;



#[test]
fn trait_values_run_as_interpreted() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("dyn");
    let cases = programs::DYN;
    for &(name, src, want) in cases {
        let (out, code) = case(&d, name, src);
        assert_eq!(code, want, "{name}: {out}");
    }
}

#[test]
fn derived_traits_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("derived");
    let (out, code) = case(&d, "derived", r#"from std.typing use Record, Field

/// Values from text, field by field.
derivable(Record) trait FromText {
  /// The fields' names.
  derived fn names(self) -> List[String] {
    comptime Self.fields.map(f -> f.name)
  }
  /// A value with every field at its zero.
  derived fn zero() -> Self {
    Self.make(f -> zero_of(f))
  }
  /// A value read from one text per field.
  derived fn parse(texts: List[String]) -> Result[Self, String] {
    Self.try_make(f -> read(f, texts))
  }
}

fn zero_of(comptime f: Field) -> f.type {
  match comptime f.type {
    Int => 0,
    String => "",
    Bool => false,
  }
}

fn read(comptime f: Field, texts: List[String]) -> Result[f.type, String] {
  text := get(texts, index_of_name(comptime f.name)) or { "" }
  match comptime f.type {
    Int => match parse_int(text) {
      Some(n) => Ok(n),
      None => Err(f"{f.name}: `{text}` is not a number"),
    },
    String => Ok(text),
    Bool => Ok(text == "yes"),
  }
}

fn index_of_name(name: String) -> Int {
  if name == "x" {
    0
  } else if name == "label" {
    1
  } else {
    2
  }
}

type Point = { x: Int, label: String, on: Bool }

derive FromText for Point

fn main() {
  print(Point.zero().names())
  print(Point.zero())
  print(Point.parse(["3", "here", "yes"]))
  print(Point.parse(["three", "here", "yes"]))
}
"#);
    assert!(out.contains("is not a number") && code == 0, "{out}");
}

#[test]
fn derived_for_sums_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("derived-sums");
    // `comptime for`, a match comprehension, `v.make()`
    let (out, code) = case(&d, "sums", r#"from std.typing use Choice, Union

/// A choice and its name.
derivable(Choice) trait Named {
  derived fn from_name(name: String) -> Option[Self] {
    comptime for v in Self.members {
      if to_lower(v.name ?? "") == name {
        return Some(v.make())
      }
    }
    None
  }
  derived fn name(self) -> String {
    match self {
      v => to_lower(v.name ?? "") for v in Self.members
    }
  }
}

/// How many fields each variant has.
derivable(Union) trait Width
  requires len(Self.members) > 0 {
  derived fn width(self) -> Int {
    match self {
      v => len(v.fields) + len(v.elements) for v in Self.members
    }
  }
}

type Level =
  | Quiet
  | Loud

derive Named for Level

type Shape =
  | Dot
  | Rect(Int, Int)
  | Circle { r: Int, label: String }

derive Width for Shape

fn main() {
  print(Level.from_name("loud"))
  print(Level.from_name("nope"))
  print(Loud.name())
  print([Dot, Rect(1, 2), Circle { r: 1, label: "c" }].map(s -> s.width()))
}
"#);
    assert!(out.contains("Some(Loud)\nNone\nloud\n[0, 2, 2]") && code == 0, "{out}");
}

/// The argument sets the examples with options are run with: values,
/// defaults, `--help`, and bad arguments (exit code 2).
const OPTION_RUNS: &[(&str, &[&str])] = &[
    ("hello", &[]),
    ("hello", &["Ada", "--loud"]),
    ("hello", &["--times", "2", "Ada", "Bob"]),
    ("hello", &["--times=3"]),
    ("hello", &["--times", "30"]),
    ("hello", &["--times", "x"]),
    ("hello", &["--help"]),
    ("hello", &["-h"]),
    ("hello", &["--bogus"]),
    ("wc", &["--help"]),
    ("wc", &["--only", "lines", "examples/wc/strata.toml", "README.md"]),
    ("wc", &["--only", "bytes"]),
    ("wc", &["examples/wc/strata.toml"]),
];

/// `main(opts: Options)`: a compiled program reads its options with the
/// derived `from_args`, as `strata run` does (same output, same exit code).
#[test]
fn options_from_the_command_line_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("options");
    let strata = env!("CARGO_BIN_EXE_strata");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // named after the package, as `strata build` names it (`--help` shows the name)
    for ex in ["hello", "wc"] {
        let b = Command::new(strata).arg("build").arg(root.join("examples").join(ex)).arg("-o").arg(d.join(ex)).output().unwrap();
        assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    }
    let mut codes = vec![];
    for (i, (ex, args)) in OPTION_RUNS.iter().enumerate() {
        let native = output(Command::new(d.join(ex)).args(*args).current_dir(root).stdin(Stdio::null()));
        expected(&format!("options-{i}-{ex}"), &native);
        codes.push(native.1);
    }
    assert_eq!(codes, [0, 0, 0, 0, 2, 2, 0, 0, 2, 0, 0, 2, 0]);
    // a mutable options parameter, in a script
    let f = d.join("opts.strata");
    save(&f, "from std.cli use CommandLine\n\ntype Options = {\n  /// How many.\n  count: Option[Int],\n  names: List[String],\n}\n\nderive CommandLine for Options\n\nfn main(mut opts: Options) {\n  opts.names = opts.names + [\"end\"]\n  print(f\"{opts.count} {opts.names}\")\n}\n");
    let exe = d.join("opts");
    let b = Command::new(strata).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    for (i, args) in [&["--count", "4", "a", "b"][..], &[], &["--count"], &["--help"]].iter().enumerate() {
        let native = output(Command::new(&exe).args(*args));
        expected(&format!("options-script-{i}"), &native);
    }
}

#[test]
fn what_is_not_compiled_yet_is_named() {
    let d = tmp("unsupported");
    let f = d.join("many.strata");
    save(&f, "trait Many {\n  fn copies(self) -> List[Self]\n}\n\ntype A = { n: Int }\n\nimpl Many for A {\n  fn copies(self) -> List[A] { [self, self] }\n}\n\nfn main() {\n  let m: Many = A { n: 1 }\n  print(len(m.copies()))\n}\n");
    let o = Command::new(env!("CARGO_BIN_EXE_strata")).arg("build").arg(&f).arg("--emit-llvm").output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    let why = "can't compile `main`: calling `copies` (it mentions `Self` inside another type) on a value of the trait type `Many` is not compiled yet";
    assert!(!o.status.success() && err.contains(why), "{err}");
    // `strata run` says so too (nothing is interpreted instead)
    let o = Command::new(env!("CARGO_BIN_EXE_strata")).arg("run").arg(&f).output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success() && err.contains(&format!("the program can't be compiled yet: {why}")), "{err}");
}

#[test]
fn emits_llvm_ir() {
    let d = tmp("emit");
    let f = d.join("p.strata");
    save(&f, "fn twice(n: Int) -> Int { n * 2 }\nfn main() -> () { print(twice(21)) }\n");
    let o = Command::new(env!("CARGO_BIN_EXE_strata")).arg("build").arg(&f).arg("--emit-llvm").output().unwrap();
    let ir = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(ir.contains("define i64 @\"strata:twice\"(i64 %p.n)") && ir.contains("define i32 @main(i32 %argc, ptr %argv)"), "{ir}");
}

/// `strata run --jit`: compiled in memory, the expected output.
#[test]
fn the_jit_runs_as_the_interpreter() {
    let d = tmp("jit");
    let strata = env!("CARGO_BIN_EXE_strata");
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/build/numbers.strata");
    let jit = output(Command::new(strata).args(["run", "--jit"]).arg(&file));
    expected("jit-numbers", &jit);
    assert_eq!(jit.1, 101);
    // a pattern built at run time: std.regex's engine, in a built program too
    // (its messages are its own, not the interpreter's Rust `regex`'s)
    let f = d.join("lists.strata");
    save(&f, "fn main() { p := \"[a-z]\" + \"[0-9]\" print(matches(\"a1\", p)) print(matches(\"a\", p + \"(\")) }\n");
    let jit = output(Command::new(strata).args(["run", "--jit"]).arg(&f));
    expected("jit-regex", &jit);
    assert!(jit.0.starts_with("true\nfault (Runtime): bad regex: unclosed group") && jit.1 == 101, "{jit:?}");
    let exe = d.join("lists");
    assert!(Command::new(strata).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap().status.success());
    assert_eq!(output(&mut Command::new(&exe)), jit);
}

/// `strata run --jit` with options: the derived `from_args`, compiled.
#[test]
fn the_jit_reads_options() {
    let strata = env!("CARGO_BIN_EXE_strata");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // what the built programs give (`options_from_the_command_line_compile`)
    for (i, (ex, args)) in OPTION_RUNS.iter().enumerate() {
        let jit = output(Command::new(strata).args(["run", "--jit"]).arg(root.join("examples").join(ex)).arg("--").args(*args).current_dir(root).stdin(Stdio::null()));
        assert!(!jit.0.contains("note: interpreted"), "{}", jit.0);
        expected(&format!("options-{i}-{ex}"), &jit);
    }
}

/// Every compiled program frees what it made (`built_as_expected` checks
/// it): one that goes through closures, trait values, `mut`, patterns,
/// early exits, collections and strings, and a chain too long to drop
/// recursively.
#[test]
fn memory_is_freed() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("memory");
    let (out, code) = case(&d, "features", MEMORY);
    assert!(code == 0 && out.contains("Named { name: \"new\", tags: [\"t\", \"renamed\"] }"), "{out}");
    // an operand that borrows a variable a later operand assigns
    let (out, code) = case(&d, "nested_assign", NESTED_ASSIGN);
    assert!(code == 0 && out.starts_with("first 1/1\nsecond 2\n"), "{out}");
    // values that don't outlive their variable, on the stack: their fields
    // (strings, a record, a list, a big integer) dropped with the variable,
    // on every way out of its block
    let f = d.join("stack.strata");
    save(&f, STACK_VALUES);
    let ir = Command::new(env!("CARGO_BIN_EXE_strata")).arg("build").arg(&f).arg("--emit-llvm").output().unwrap();
    let ir = String::from_utf8_lossy(&ir.stdout);
    assert_eq!(ir.matches(" = alloca [").count(), 2, "the values that stay in their block, on the stack:\n{ir}");
    let (out, code) = case(&d, "stack", STACK_VALUES);
    assert!(code == 0 && out.ends_with("15840\n1\nfound Ada7 at 1\n"), "{out}");
    // a field assigned in place when nothing else holds the value, in a
    // copy when something does (another variable, a list, an operand, a
    // task, a constant)
    let f = d.join("in_place.strata");
    save(&f, IN_PLACE);
    let ir = Command::new(env!("CARGO_BIN_EXE_strata")).arg("build").arg(&f).arg("--emit-llvm").output().unwrap();
    assert!(String::from_utf8_lossy(&ir.stdout).contains("@strata_unique(ptr"), "no assignment in place");
    let (out, code) = case(&d, "in_place", IN_PLACE);
    assert!(code == 0 && out.ends_with("done\n"), "{out}");
    // dictionaries and sets changed in place by `x = insert(x, …)` when
    // nothing else holds them, copied when something does
    let f = d.join("insert_in_place.strata");
    save(&f, INSERT_IN_PLACE);
    let ir = Command::new(env!("CARGO_BIN_EXE_strata")).arg("build").arg(&f).arg("--emit-llvm").output().unwrap();
    assert!(String::from_utf8_lossy(&ir.stdout).contains("@strata_list_owned(ptr"), "no insertion in place");
    let (out, code) = case(&d, "insert_in_place", INSERT_IN_PLACE);
    assert!(code == 0 && out.ends_with("done\n"), "{out}");
    // constructors passed to a function that only reads them, on the stack;
    // dictionaries and sets with elements removed in place
    let f = d.join("more_in_place.strata");
    save(&f, MORE_IN_PLACE);
    let ir = Command::new(env!("CARGO_BIN_EXE_strata")).arg("build").arg(&f).arg("--emit-llvm").output().unwrap();
    let ir = String::from_utf8_lossy(&ir.stdout);
    assert_eq!(ir.matches(" = alloca [").count(), 4, "the arguments on the stack:\n{ir}");
    assert!(ir.contains("@strata_list_remove_at(ptr"), "no removal in place");
    let (out, code) = case(&d, "more_in_place", MORE_IN_PLACE);
    assert!(code == 0 && out.ends_with("done\n"), "{out}");
    let chain = "type Chain = | End | Link(Int, Chain)\nfn main() { mut c: Chain = End mut i := 0 while i < 1000000 { c = Link(i, c) i += 1 } match c { Link(x, _) => print(x), End => print(0) } }\n";
    // natively only: the interpreter takes minutes on this loop
    let f = d.join("chain.strata");
    save(&f, chain);
    let exe = d.join("chain");
    let b = Command::new(env!("CARGO_BIN_EXE_strata")).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
    let (out, code) = freed_everything(output(Command::new(&exe).env("STRATA_MEMORY", "check")), &f);
    assert!(code == 0 && out == "999999\n", "{out}");
}

/// Tasks (`scope`, `spawn`, `await`) on threads, compiled and on the JIT:
/// results, faults, early exits, and counts shared between threads (many
/// tasks holding and dropping the same captured lists), everything freed.
#[test]
fn tasks_run_on_threads() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("tasks");
    let (out, code) = case(&d, "tasks", TASKS);
    assert!(code == 0 && out.starts_with("75025 6768\n"), "{out}");
    let jit = freed_everything(output(Command::new(env!("CARGO_BIN_EXE_strata")).args(["run", "--jit"]).arg(d.join("tasks.strata")).env("STRATA_MEMORY", "check")), &d.join("tasks.strata"));
    assert_eq!(jit, (out, code));
    // a panic in a task ends the program at once: the main thread, waiting
    // for values no one will send, doesn't hang
    let (out, code) = case(&d, "fault", "fn pick(xs: List[Int], i: Int) -> Int { xs[i] }\nfn main() {\n  (tx, rx) := channel()\n  scope {\n    spawn pick([1, 2], 5)\n    for x in rx {\n      print(x)\n    }\n  }\n  tx.close()\n}\n");
    assert!(code == 101 && out.contains("fault (Runtime): index 5 out of range (length 2)"), "{out}");
}

/// Channels and `select`, compiled and on the JIT: a pool of workers, a
/// `select` over two receivers (one unbuffered), one over a task and a
/// send; a send after the channel ended, and a task that didn't close its
/// copy of a sender, fault.
#[test]
fn channels_run_on_threads() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("channels");
    let (out, code) = case(&d, "channels", CHANNELS);
    assert!(code == 0 && out == "total 385\n10 [\"a\", \"b\", \"c\"]\nticked true\n", "{out}");
    let jit = freed_everything(output(Command::new(env!("CARGO_BIN_EXE_strata")).args(["run", "--jit"]).arg(d.join("channels.strata")).env("STRATA_MEMORY", "check")), &d.join("channels.strata"));
    assert_eq!(jit, (out, code));
    let (out, code) = case(&d, "ended", "fn main() {\n  (tx, rx) := channel(1)\n  tx.close()\n  scope {\n    spawn {\n      tx.send(5)\n      tx.close()\n    }\n  }\n}\n");
    assert!(code == 101 && out.contains("`send` on a channel that has ended"), "{out}");
    let (out, code) = case(&d, "unclosed", "fn main() {\n  (tx, rx) := channel(1)\n  scope {\n    spawn {\n      tx.send(1)\n      if len([1, 2]) > 5 {\n        tx.close()\n      }\n    }\n  }\n  tx.close()\n  for x in rx {\n    print(x)\n  }\n}\n");
    assert!(code == 101 && out.contains("a task ended without closing its copy of a sender"), "{out}");
}

/// Run `c` with stdin given `input`, then (if `close`) closed, or else a pipe
/// held open with nothing more written: a read of it then waits for ever
/// unless it is cancelled. Killed (and the test failed) after `secs`.
fn output_with_stdin(c: &mut Command, input: &str, close: bool, secs: u64) -> (String, i32) {
    let mut child = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(input.as_bytes()).unwrap();
    let held = if close { drop(stdin); None } else { Some(stdin) };
    let start = std::time::Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed().as_secs() >= secs {
            let _ = child.kill();
            panic!("still waiting for input after {secs} s: the read wasn't cancelled");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    drop(held);
    let o = child.wait_with_output().unwrap();
    let err: String = String::from_utf8_lossy(&o.stderr).lines().filter(|l| !l.starts_with("    stack:")).map(|l| format!("{l}\n")).collect();
    (String::from_utf8_lossy(&o.stdout).to_string() + &err, o.status.code().unwrap_or(-1))
}

/// A task waiting for input is cancelled like any waiting task: a `race`
/// whose loser waits in `read_line` on a stdin held open (nothing written)
/// ends at once, 20 times; so does a supervised task's reader when its
/// sibling panics. A task's `read_line` (the cancellable one) reads the
/// lines given, the second and third from what the first read buffered.
/// Natively and on the JIT, everything freed.
#[test]
fn reads_are_cancelled() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("io-cancel");
    let strata = env!("CARGO_BIN_EXE_strata");
    for (name, src, input, close, want) in [
        ("blocked", IO_BLOCKED, "", false, "the reader lost 20 of 20 races\ncontained: index 3 out of range (length 1)\n"),
        ("lines", IO_LINES, "first\nsecond\n", true, "first | second | the end of the input\n"),
    ] {
        let f = d.join(format!("{name}.strata"));
        save(&f, src);
        let exe = d.join(name);
        let b = Command::new(strata).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
        assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
        let native = freed_everything(output_with_stdin(Command::new(&exe).env("STRATA_MEMORY", "check"), input, close, 20), &f);
        assert_eq!(native, (want.to_string(), 0), "{name}");
        let jit = freed_everything(output_with_stdin(Command::new(strata).args(["run", "--jit"]).arg(&f).env("STRATA_MEMORY", "check"), input, close, 60), &f);
        assert_eq!(jit, native, "{name}");
    }
}

const IO_BLOCKED: &str = r#"from std.io use Console


fn line_of(c: Console) -> String {
  mut d := c
  mut d.read_line() ?? "the end of the input"
}

fn pick(xs: List[Int], i: Int) -> Int {
  xs[i]
}

fn reads_and_fails(c: Console) -> String {
  scope {
    r := spawn line_of(c)
    p := spawn pick([1], 3)
    f"{await r} {await p}"
  }
}

fn main(console: Console) {
  mut lost := 0
  for i in 0..20 {
    r := race {
      line_of(console) as line => f"read {line}",
      [f"{i}", "computed"] as xs => xs[1],
    }
    if r == "computed" {
      lost += 1
    }
  }
  print(f"the reader lost {lost} of 20 races")
  s := scope {
    h := supervised spawn reads_and_fails(console)
    match await h {
      Ok(v) => v,
      Err(p) => f"contained: {p.message}",
    }
  }
  print(s)
}
"#;

const IO_LINES: &str = r#"from std.io use Console

fn line_of(c: Console) -> String {
  mut d := c
  mut d.read_line() ?? "the end of the input"
}

fn main(console: Console) {
  a := scope {
    h := spawn line_of(console)
    await h
  }
  b := scope {
    h := spawn line_of(console)
    await h
  }
  c := scope {
    h := spawn line_of(console)
    await h
  }
  print(f"{a} | {b} | {c}")
}
"#;

/// `race`: the first arm to finish wins; the others are cancelled at
/// their next wait (deep in calls at a `recv`, at a `send` to a full channel,
/// at an `await` of their own task, in a `select`, in a race of their own)
/// and leave as `return` would, so everything they held is freed.
#[test]
fn races_cancel_the_losers() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("race");
    let (out, code) = case(&d, "race", RACE);
    assert!(code == 0 && out == "answer 42\nwon 100 of 100\n1 left in the channel\n", "{out}");
    let jit = freed_everything(output(Command::new(env!("CARGO_BIN_EXE_strata")).args(["run", "--jit"]).arg(d.join("race.strata")).env("STRATA_MEMORY", "check")), &d.join("race.strata"));
    assert_eq!(jit, (out, code));
}

/// `supervised spawn`: a panic ends only that task, and awaiting it gives
/// `Err(Panic { … })`: deep in calls, a broken `requires`, a dead task's
/// sender copy closed (its receiver's loop ends), an ordinary task's panic
/// below a supervised one, `await h except p { … }`, a supervised `race`
/// arm; everything the dying tasks held is freed, the runtime's builtins'
/// panics too (a division by zero, a negative shift, `sqrt`, `ord`, `chr`,
/// a byte out of range, a float that isn't finite: deep in calls, every
/// frame holding lists, 50 times).
#[test]
fn supervised_tasks_keep_running() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("supervised");
    let (out, code) = case(&d, "supervised", SUPERVISED);
    assert!(code == 0 && out.contains("50 of 50 panicked\nfell back after: precondition of `half` violated") && out.ends_with("lost: Runtime\n"), "{out}");
    let jit = freed_everything(output(Command::new(env!("CARGO_BIN_EXE_strata")).args(["run", "--jit"]).arg(d.join("supervised.strata")).env("STRATA_MEMORY", "check")), &d.join("supervised.strata"));
    assert_eq!(jit, (out, code));
    let (out, code) = case(&d, "builtins", SUPERVISED_BUILTINS);
    assert!(code == 0 && out.contains("Runtime: division by zero\n") && out.contains("Runtime: 11 / 0 is not a finite number\n") && out.ends_with("50 of 50 panicked, the program goes on\n"), "{out}");
    let jit = freed_everything(output(Command::new(env!("CARGO_BIN_EXE_strata")).args(["run", "--jit"]).arg(d.join("builtins.strata")).env("STRATA_MEMORY", "check")), &d.join("builtins.strata"));
    assert_eq!(jit, (out, code));
    // a stack overflow can't return: the task still dies alone (its `await`
    // gives the panic), but what its frames held stays allocated (IDEAS.md)
    let f = d.join("overflow.strata");
    save(&f, SUPERVISED_OVERFLOW);
    let (out, code) = output(Command::new(env!("CARGO_BIN_EXE_strata")).args(["run", "--jit"]).arg(&f).env("STRATA_MEMORY", "check"));
    assert!(code == 0 && out.contains("Limit: stack overflow (recursion too deep)\nstill running\n") && !out.contains(" 0 live"), "{out}");
}

const SUPERVISED_OVERFLOW: &str = r#"fn down(n: Int, xs: List[String]) -> Int {
  ys := [f"{n}"]
  down(n + 1, xs) + len(ys)
}

fn main() {
  r := scope {
    h := supervised spawn down(0, [])
    await h
  }
  match r {
    Ok(v) => print(v),
    Err(p) => print(f"{p.kind}: {p.message}"),
  }
  print("still running")
}
"#;

const SUPERVISED: &str = r#"fn pick(xs: List[Int], i: Int) -> Int {
  ys := [f"held {i}", "more"]
  if i > 3 {
    xs[i] + len(ys)
  } else {
    pick(xs, i + 1) + len(ys)
  }
}

fn half(n: Int) -> Int
  requires n % 2 == 0 {
  n / 2
}

fn shown(r: Result[Int, Panic]) -> String {
  match r {
    Ok(v) => f"ok {v}",
    Err(p) => f"panicked ({p.kind}): {p.message} at {p.stack}",
  }
}

fn main() {
  (a, b, c) := scope {
    x := supervised spawn pick([1, 2], 0)
    y := supervised spawn 40 + 2
    z := supervised spawn half(3)
    await (x, y, z)
  }
  print(shown(a))
  print(shown(b))
  print(shown(c))
  // a dead task's copy of a sender is closed: the receiver's loop ends
  (tx, rx) := channel(4)
  (r, got) := scope {
    h := supervised spawn {
      tx.send("one")
      tx.send(f"two {pick([1], 0)}")
      tx.close()
      0
    }
    tx.close()
    mut got := []
    for s in rx {
      got += [s]
    }
    (await h, got)
  }
  print(f"{shown(r)}; received {got}")
  // a panic in an ordinary task below a supervised one is the supervised one's
  d := scope {
    h := supervised spawn scope {
      t := spawn pick([1], 0)
      await t + 1
    }
    await h
  }
  print(shown(d))
  // many, all freed
  mut n := 0
  for i in 0..50 {
    r := scope {
      h := supervised spawn pick([i], 0)
      await h
    }
    if r is Err {
      n += 1
    }
  }
  print(f"{n} of 50 panicked")
  // handled where it is awaited: `except` on the `Result`
  e := scope {
    h := supervised spawn half(5)
    await h except p {
      print(f"fell back after: {p.message}")
      0
    }
  }
  print(e)
  // a supervised race arm: its panic is its result, and the race goes on
  (hold, wait) := channel()
  w := race {
    supervised pick([1], 0) as r => match r {
      Ok(v) => f"won with {v}",
      Err(p) => f"lost: {p.kind}",
    },
    wait.recv() ?? "never" as x => x,
  }
  hold.close()
  print(w)
}
"#;

const SUPERVISED_BUILTINS: &str = r#"fn deep(xs: List[String], n: Int, k: Int) -> Int {
  ys := [f"frame {n}", "held"]
  if n == 0 {
    boom(k, xs) + len(ys)
  } else {
    deep(xs + ys, n - 1, k) + len(ys)
  }
}

fn boom(k: Int, xs: List[String]) -> Int {
  zero := len(xs) - len(xs)
  match k % 8 {
    0 => len(xs) / zero,
    1 => len(xs) % zero,
    2 => 1 << (zero - 1),
    3 => floor(sqrt(float(zero - 1))),
    4 => ord(xs[0]),
    5 => byte_len(chr(zero - 1)),
    6 => byte_at(xs[0], 100),
    _ => floor(float(len(xs)) / float(zero)),
  }
}

fn main() {
  mut n := 0
  for i in 0..50 {
    r := scope {
      h := supervised spawn deep([f"start {i}"], 5, i)
      await h
    }
    match r {
      Ok(v) => print(f"no panic: {v}"),
      Err(p) => {
        n += 1
        if i < 8 {
          print(f"{p.kind}: {p.message}")
        }
      },
    }
  }
  print(f"{n} of 50 panicked, the program goes on")
}
"#;

const RACE: &str = r#"fn hold_and_wait(rx: Receiver[String], depth: Int) -> String {
  xs := [f"level {depth}", "held"]
  if depth == 0 {
    rx.recv() ?? "ended"
  } else {
    hold_and_wait(rx, depth - 1) + xs[0]
  }
}

fn answer() -> Int {
  6 * 7
}

fn inner(rx: Receiver[String]) -> String {
  race {
    hold_and_wait(rx, 2) as s => s,
    hold_and_wait(rx, 3) as t => t,
  }
}

fn main() {
  (tx, rx) := channel()
  (full, sink) := channel(1)
  r := race {
    hold_and_wait(rx, 5) as s => f"waited {s}",
    answer() as n => f"answer {n}",
  }
  print(r)
  mut wins := 0
  for i in 0..100 {
    w := race {
      {
        full.send(f"v{i}")
        full.send(f"w{i}")
        full.close()
        "sent"
      } as a => a,
      scope {
        h := spawn hold_and_wait(rx, 3)
        await h
      } as b => b,
      select {
        rx.recv() as x => x ?? "none",
      } as c => c,
      inner(rx) as d => d,
      [f"{i}", "won"] as e => e[1],
    }
    if w == "won" {
      wins += 1
    }
  }
  print(f"won {wins} of 100")
  full.close()
  tx.close()
  mut n := 0
  for x in sink {
    n += 1
  }
  print(f"{n} left in the channel")
}
"#;

/// std.time's clock: a `race` as a timeout (the loser cancelled while it
/// waits on a channel), a five-second sleep cancelled by a quicker arm, a
/// `select` with an `after` arm; everything freed, and the time taken shows
/// the long sleep was cut short.
#[test]
fn clocks_time_out() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("clock");
    let (out, code) = case(&d, "clock", CLOCK);
    assert!(code == 0 && out == "None\n42\ntimed out\ntimeouts 20\ntrue\n", "{out}");
    let jit = freed_everything(output(Command::new(env!("CARGO_BIN_EXE_strata")).args(["run", "--jit"]).arg(d.join("clock.strata")).env("STRATA_MEMORY", "check")), &d.join("clock.strata"));
    assert_eq!(jit, (out, code));
}

/// `after` follows the clock it is called on: a frozen fake clock's timer
/// never fires though real time passes (the `recv` wins), one of 0 fires at
/// once, a clock 1000 times faster fires its ten seconds in about 10 ms, and
/// the system clock passed as a `Clock` works too; everything freed.
#[test]
fn fake_clocks_drive_after() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("fake-clock");
    let (out, code) = case(&d, "fake-clock", FAKE_CLOCK);
    assert!(code == 0 && out == "received\nfrozen timer at 0\nfast timer\ntrue\ntimer\n", "{out}");
    let jit = freed_everything(output(Command::new(env!("CARGO_BIN_EXE_strata")).args(["run", "--jit"]).arg(d.join("fake-clock.strata")).env("STRATA_MEMORY", "check")), &d.join("fake-clock.strata"));
    assert_eq!(jit, (out, code));
}

/// When every task waits and none of the waits ends by itself, nothing can
/// happen any more: a deadlock is reported (a panic naming each wait), not a
/// silent hang; a real clock's sleep or a long computation isn't one. A
/// virtual clock's time jumps to the nearest deadline once every task waits:
/// a timeout, sleeps in the order of their deadlines, an `after` arm, with no
/// task moving the time.
#[test]
fn waits_are_known() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("waits");
    let (out, code) = case(&d, "deadlock", DEADLOCK);
    assert!(code == 101 && out.contains("deadlock: every task is waiting, so none can go on: main waits in the end of a `scope`, at main; a task waits in `recv`"), "{out}");
    let (jit_out, jit_code) = output(Command::new(env!("CARGO_BIN_EXE_strata")).args(["run", "--jit"]).arg(d.join("deadlock.strata")));
    assert!(jit_code == 101 && jit_out.contains("deadlock: every task is waiting"), "{jit_out}");
    for (name, src, want) in [("not-deadlocked", NOT_DEADLOCKED, "1\n196418\n"), ("virtual-time", VIRTUAL_TIME, "timed out\n30\none\ntwo\nthree\n33\nafter 10 s\n43\n")] {
        let (out, code) = case(&d, name, src);
        assert!(code == 0 && out == want, "{name}: {out}");
        let jit = freed_everything(output(Command::new(env!("CARGO_BIN_EXE_strata")).args(["run", "--jit"]).arg(d.join(format!("{name}.strata"))).env("STRATA_MEMORY", "check")), &d.join(format!("{name}.strata")));
        assert_eq!(jit, (out, code), "{name}");
    }
}

const DEADLOCK: &str = r#"fn main() {
  (atx, arx) := channel()
  (btx, brx) := channel()
  scope {
    spawn {
      x := brx.recv() ?? 0
      atx.send(x)
      atx.close()
    }
    spawn {
      y := arx.recv() ?? 0
      btx.send(y)
      btx.close()
    }
  }
  atx.close()
  btx.close()
}
"#;

const NOT_DEADLOCKED: &str = r#"from std.time use SystemClock, ms

fn fib(n: Int) -> Int {
  if n < 2 {
    n
  } else {
    fib(n - 1) + fib(n - 2)
  }
}

fn main(clock: SystemClock) {
  (tx, rx) := channel()
  // a real sleep while the main thread waits: not a deadlock
  scope {
    spawn {
      clock.sleep(ms(100))
      tx.send(1)
      tx.close()
    }
    tx.close()
    print(rx.recv() ?? 0)
  }
  // a long computation while the main thread waits: not a deadlock
  r := scope {
    h := spawn fib(27)
    await h
  }
  print(r)
}
"#;

const VIRTUAL_TIME: &str = r#"from std.time use VirtualClock, seconds

fn answer(rx: Receiver[Int]) -> Int {
  rx.recv() ?? 0
}

fn main(clock: VirtualClock) {
  (tx, rx) := channel()
  // a timeout no task moves the time for
  r := race {
    answer(rx) as v => f"answer {v}",
    clock.sleep(seconds(30)) => "timed out",
  }
  print(r)
  print(clock.now().value / 1000000000)
  // sleeps end in the order of their deadlines
  scope {
    spawn {
      clock.sleep(seconds(3))
      print("three")
    }
    spawn {
      clock.sleep(seconds(1))
      print("one")
    }
    spawn {
      clock.sleep(seconds(2))
      print("two")
    }
  }
  print(clock.now().value / 1000000000)
  // an `after` arm
  s := select {
    rx.recv() as v => f"got {v ?? 0}",
    clock.after(seconds(10)) => "after 10 s",
  }
  print(s)
  print(clock.now().value / 1000000000)
  tx.close()
}
"#;

const FAKE_CLOCK: &str = r#"from std.time use SystemClock, Clock, Instant, Duration, nanos, ms, seconds, elapsed

/// A clock whose time never moves.
type FrozenClock = { at: Int }

impl Clock for FrozenClock {
  fn now(self) -> Instant {
    Instant(self.at)
  }

  fn sleep(self, d: Duration) {
    ()
  }
}

/// A clock running `speed` times faster than the system's.
type FastClock = { inner: SystemClock, speed: Int }

impl Clock for FastClock {
  fn now(self) -> Instant {
    Instant(self.inner.now().value * self.speed)
  }

  fn sleep(self, d: Duration) {
    self.inner.sleep(nanos(d.value / self.speed))
  }
}

/// Any clock: here the system's, through the `Clock` trait.
fn timer_of(clock: Clock, rx: Receiver[Int]) -> String {
  select {
    rx.recv() as x => "received",
    clock.after(ms(20)) => "timer",
  }
}

fn main(clock: SystemClock) {
  (tx, rx) := channel()
  start := clock.now()
  // the frozen clock's 1 ms never passes, though 50 ms of real time do
  frozen := FrozenClock { at: 5 }
  r := scope {
    spawn {
      clock.sleep(ms(50))
      tx.send(1)
      tx.close()
    }
    select {
      rx.recv() as x => "received",
      frozen.after(ms(1)) => "frozen timer",
    }
  }
  print(r)
  // a timer of 0 is ready at once, in any clock's time
  print(select {
    rx.recv() as x => "received",
    frozen.after(nanos(0)) => "frozen timer at 0",
  })
  // ten seconds of a clock 1000 times faster are 10 ms of the system's
  fast := FastClock { inner: clock, speed: 1000 }
  before := clock.now()
  print(select {
    rx.recv() as x => "received",
    fast.after(seconds(10)) => "fast timer",
  })
  print(elapsed(before, clock.now()).value < 2000000000)
  print(timer_of(clock, rx))
  tx.close()
}
"#;

const CLOCK: &str = r#"from std.time use SystemClock, Clock, ms, seconds, elapsed

fn never(rx: Receiver[Int]) -> Int {
  xs := ["held", "too"]
  (rx.recv() ?? 0) + len(xs)
}

fn work(n: Int) -> Int {
  n * 2
}

fn timeout(clock: SystemClock, rx: Receiver[Int]) -> Option[Int] {
  race {
    never(rx) as r => Some(r),
    clock.sleep(ms(30)) => None,
  }
}

fn main(clock: SystemClock) {
  (tx, rx) := channel()
  start := clock.now()
  print(timeout(clock, rx))
  r := race {
    work(21) as n => n,
    {
      clock.sleep(seconds(5))
      0
    } as z => z,
  }
  print(r)
  s := select {
    rx.recv() as x => "got",
    clock.after(ms(20)) => "timed out",
  }
  print(s)
  mut timeouts := 0
  for i in 0..20 {
    if timeout(clock, rx) is None {
      timeouts += 1
    }
  }
  print(f"timeouts {timeouts}")
  took := elapsed(start, clock.now()).value
  print(took >= 650000000 && took < 4000000000)
  tx.close()
}
"#;

const CHANNELS: &str = r#"fn work(n: Int) -> Int {
  n * n
}

fn slow(n: Int) -> Int {
  if n < 2 { n } else { slow(n - 1) + slow(n - 2) }
}

fn pool() {
  (tx, rx) := channel(4)
  (out, results) := channel()
  scope {
    spawn {
      for n in 1..=10 {
        tx.send(n)
      }
      tx.close()
    }
    for _ in 0..3 {
      spawn {
        for n in rx {
          out.send(work(n))
        }
        out.close()
      }
    }
    tx.close()
    out.close()
    mut total := 0
    for r in results {
      total += r
    }
    print(f"total {total}")
  }
}

fn merge() {
  (nums, nums_rx) := channel()
  (words, words_rx) := channel(2)
  scope {
    spawn {
      for i in 0..5 {
        nums.send(i)
      }
      nums.close()
    }
    spawn {
      for w in ["a", "b", "c"] {
        words.send(w)
      }
      words.close()
    }
    nums.close()
    words.close()
    mut got_nums := 0
    mut got_words: List[String] = []
    mut nums_open := true
    mut words_open := true
    while nums_open || words_open {
      select {
        nums_rx.recv() as n => match n {
          Some(x) => {
            got_nums += x
          },
          None => {
            nums_open = false
          },
        },
        words_rx.recv() as w => match w {
          Some(x) => {
            got_words += [x]
          },
          None => {
            words_open = false
          },
        },
      }
    }
    print(f"{got_nums} {got_words.sort()}")
  }
}

fn ticks() {
  r := scope {
    t := spawn slow(22)
    (tx, rx) := channel(1)
    mut ticks := 0
    loop {
      select {
        await t as v => {
          tx.close()
          break
        },
        tx.send(ticks) => {
          ticks += 1
          _ := rx.recv()
        },
      }
    }
    ticks > 0
  }
  print(f"ticked {r}")
}

fn main() {
  pool()
  merge()
  ticks()
}
"#;

const TASKS: &str = r#"fn fib(n: Int) -> Int {
  if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}

/// Stops at the first square over 25, waiting for the tasks started so far.
fn first_big(xs: List[Int]) -> Option[Int] {
  scope {
    for x in xs {
      h := spawn x * x
      if await h > 25 {
        return Some(x)
      }
    }
    None
  }
}

fn nested(n: Int) -> Int {
  scope {
    a := spawn scope {
      b := spawn n + 1
      await b * 2
    }
    await a
  }
}

/// Every task holds the same list and the same strings, copies parts of
/// them and drops them: the counts change on all threads at once.
fn shared(words: List[String], rounds: Int) -> Int {
  scope {
    hs := [spawn {
      mut kept: List[String] = []
      for r in 0..rounds {
        ys := words
        kept = kept + [ys[(r + k) % len(ys)]]
        if len(kept) > 8 {
          kept = kept[1..]
        }
      }
      len(kept) + k
    } for k in 0..8]
    (await hs).fold(0, (a, b) -> a + b)
  }
}

fn main() {
  xs := [1, 2, 3]
  (a, b) := scope {
    x := spawn fib(25)
    y := spawn len(xs) + fib(20)
    await (x, y)
  }
  print(f"{a} {b}")
  print(f"{scope { await [spawn fib(n) for n in 20..26] }}")
  print(f"{first_big([1, 3, 5, 7])}")
  print(f"{nested(4)}")
  mut total := 0
  for i in 0..4 {
    total += scope { await spawn i * i }
  }
  print(f"{total}")
  words := ["alpha", "beta", "gamma"].map(w -> w + "!")
  print(f"{shared(words, 20000)} {words}")
  scope {
    t := spawn print("from a task")
    await t
    print("after it")
  }
}
"#;

/// With the JIT, `strata run` compiles by default: what doesn't compile is
/// interpreted without a note (`--jit` gives one); compiled and interpreted,
/// `Int` grows into a big integer.
#[test]
fn run_compiles_by_default_with_the_jit() {
    let d = tmp("jit-default");
    let strata = env!("CARGO_BIN_EXE_strata");
    let f = d.join("big.strata");
    save(&f, "fn main() {\n  mut n := 1\n  for _ in 0..70 {\n    n = n * 2\n  }\n  print(n)\n}\n");
    let (out, code) = output(Command::new(strata).arg("run").arg(&f));
    assert_eq!((out.as_str(), code), ("1180591620717411303424\n", 0), "compiled");
    let f = d.join("regex.strata");
    save(&f, "fn main() { print(matches(\"a1\", \"[a-z][0-9]\")) }\n");
    assert_eq!(output(Command::new(strata).arg("run").arg(&f)), ("true\n".to_string(), 0));
}


/// Fields of unions of records (at different places in each), assigning
/// tuple fields, `==` on trait values, any pattern in a `for` over a
/// range; and the call stack a fault shows, as the interpreter's.
#[test]
fn fields_trait_equality_and_stacks_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("small-gaps");
    let (out, code) = case(&d, "fields", FIELDS);
    assert!(code == 0 && out.contains("Ci { name: \"renamed\", r: 2 }\n") && out.contains("((1, 9), \"x\")"), "{out}");
    let (out, code) = case(&d, "trait_equality", TRAIT_EQUALITY);
    assert!(code == 0 && out.starts_with("true\ntrue\ntrue\ntrue\n") && out.ends_with("zero\nzero\n"), "{out}");
    // the stack, which `output` leaves out
    let f = d.join("stack.strata");
    save(&f, STACK);
    let exe = d.join("stack");
    let strata = env!("CARGO_BIN_EXE_strata");
    let b = Command::new(strata).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
    let native = Command::new(&exe).output().unwrap();
    let interp = Command::new(strata).arg("run").arg(&f).output().unwrap();
    let err = String::from_utf8_lossy(&native.stderr).to_string();
    assert_eq!(err, String::from_utf8_lossy(&interp.stderr));
    assert!(err.contains("    stack: main -> Thing.describe -> inner -> half\n"), "{err}");
    // running out of stack is a fault, not a crash
    let (out, code) = {
        let f = d.join("deep.strata");
        save(&f, "fn deep(n: Int) -> Int { if n == 0 { 0 } else { 1 + deep(n - 1) } }\nfn main() { print(deep(100000000)) }\n");
        let exe = d.join("deep");
        assert!(Command::new(strata).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap().status.success());
        output(&mut Command::new(&exe))
    };
    assert!(code == 101 && out.contains("fault (Limit): stack overflow"), "{out}");
}



const STACK: &str = r#"trait Describe {
  fn describe(self) -> String
}
type Thing = { n: Int }
impl Describe for Thing {
  fn describe(self) -> String { f"{inner(self.n)}" }
}
impl Thing {
  fn twice(self) -> Int { self.n * 2 }
}
fn inner(k: Int) -> Int { [1, 2].map(x -> half(x + k)).fold(0, (a, b) -> a + b) }
fn half(n: Int) -> Int requires n % 2 == 0 { n / 2 }
fn main() {
  print(Thing { n: 2 }.twice())
  print(Thing { n: 1 }.describe())
}
"#;

/// Unions with members that aren't records or variants (`Int | String`):
/// boxed members, narrowing in arms, type and literal patterns, `==`,
/// printing, trait methods on a union, refinement checks of a union.
#[test]
fn unions_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("unions");
    let (out, code) = case(&d, "unions", UNIONS);
    assert!(code == 0 && out.starts_with("number 4\ntext 4\nzero\npositive 5\nnegative\n") && out.contains("[1, \"a\", 2]"), "{out}");
    let (out, code) = case(&d, "union_methods", UNION_METHODS);
    assert!(code == 101 && out.contains("-1 is not a `Positive | String`"), "{out}");
}

#[test]
fn declared_unions_and_parametric_type_patterns() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("declared_unions");
    let (out, code) = case(&d, "declared", DECLARED_UNIONS);
    assert_eq!((out.as_str(), code), ("text\n3\nlist 2\ntext\nempty\n", 0));
}




/// Methods called on trait values that mention `Self`, take a `mut`
/// parameter or type parameters of their own; a generic supertrait's
/// methods; a function returning a trait-typed value.
#[test]
fn trait_value_methods_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("trait-methods");
    let (out, code) = case(&d, "trait_methods", TRAIT_METHODS);
    assert!(code == 0 && out.contains("(\"sq2\", \"x\")") && out.ends_with("pretty square\n<4>\n100\ncircle\n"), "{out}");
}


/// `matches` with a literal pattern, compiled to a DFA (the interpreter's
/// engine): ASCII and Unicode classes, in a refinement too; a bad pattern
/// faults with the interpreter's message. And functions shown as values.
#[test]
fn regexes_and_shown_functions_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("regex");
    let (out, code) = case(&d, "regex", REGEX);
    assert!(code == 101 && out.starts_with("abc: true\nabc_d1: true\nAbc: false\n") && out.contains("does not satisfy `Sku`"), "{out}");
    let (out, code) = case(&d, "bad_regex", "fn main() { print(matches(\"a\", \"(\")) }\n");
    assert!(code == 101 && out.contains("bad regex: unclosed group"), "{out}");
    let (out, code) = case(&d, "shown_functions", SHOWN_FUNCTIONS);
    assert!(code == 0 && out.starts_with("<closure (x) -> …>\nH { name: \"h\", on: <fn twice> }\n"), "{out}");
}

const REGEX: &str = r#"type Sku = String where matches(it, "[A-Z]{3}-[0-9]{4}")

fn check(s: String) -> Bool { matches(s, "[a-z]+(_[a-z0-9]+)*") }

fn main() {
  for s in ["abc", "abc_d1", "Abc", "", "a__b", "héllo"] {
    print(f"{s}: {check(s)}")
  }
  print(matches("héllo", "\\w+"))
  print(matches("日本語", "\\p{Han}+"))
  print(matches("x", "(a|b|x)"))
  print(matches("2024-01-31", "\\d{4}-\\d{2}-\\d{2}"))
  print("ab".matches("a.?b"))
  let k: Sku = "ABC-1234"
  print(k)
  let s: String = "abc-1234"
  let bad: Sku = s
  print(bad)
}
"#;

const SHOWN_FUNCTIONS: &str = r#"fn twice(n: Int) -> Int { n * 2 }
type H = { name: String, on: (Int) -> Int }
fn main() {
  k := 3
  f := (x: Int) -> x + k
  print(f)
  print(H { name: "h", on: twice })
  print([f, (a: Int) -> a])
  print(f(1))
}
"#;

/// `Int` never overflows, compiled too: small integers are themselves, any
/// other is a big integer (runtime.c, "integers"), through arithmetic,
/// Euclidean `/` and `%`, comparisons, equality, ordering, printing,
/// `float`, `floor`, `parse_int`, literals and patterns.
#[test]
fn big_integers_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("bigint");
    let (out, code) = case(&d, "bigint", BIG_INTEGERS);
    assert!(code == 0 && out.starts_with("15511210043330985984000000\n870\n354224848179261915075\n") && out.contains("9223372036854775808\n-9223372036854775809\n"), "{out}");
}


/// `mock T` compiled: methods whose results are generated within the
/// result's refinements (bounds, lengths, regexes; rejection otherwise)
/// until `ensures` holds, after `requires` (whose fault is the
/// interpreter's). The values are not the interpreter's; what they satisfy is.
#[test]
fn mocks_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("mock");
    let (out, code) = case(&d, "mock", MOCKS);
    assert!(code == 101 && out.starts_with("<mock Store>\nall good\nfault (Requires): precondition of `Store.find` violated: requires id > 0"), "{out}");
    let (out, code) = case(&d, "orders", &tests_as_main("examples/orders.strata", ""));
    assert!(code == 0 && out.ends_with("all asserts hold\n"), "{out}");
}

const MOCKS: &str = r#"type Item = { name: NonEmptyString, qty: Positive }
type Color = | Red | Green | Rgb(Int, Int, Int)

trait Store {
  fn count(self) -> Natural
  fn find(self, id: Int) -> Option[Item]
    requires id > 0
  fn all(self) -> List[Item]
    ensures len(it) <= 3
  fn color(self) -> Color
  fn code(self) -> String where matches(it, "[A-Z]{2}[0-9]{3}")
  fn save(mut self, i: Item) -> Result[(), String]
  fn rate(self) -> Float where 0.0 <= it && it <= 1.0
}

fn main() {
  mut s := mock Store
  for _ in 0..20 {
    n := s.count()
    assert n >= 0
    match s.find(3) {
      Some(i) => { assert i.name != "" && i.qty > 0 },
      None => (),
    }
    assert len(s.all()) <= 3
    c := s.code()
    assert matches(c, "[A-Z]{2}[0-9]{3}")
    r := s.rate()
    assert 0.0 <= r && r <= 1.0
    k := s.color()
    assert k is Red || k is Green || k is Rgb
    res := mut s.save(Item { name: "a", qty: 1 })
    assert res is Ok || res is Err
  }
  print(s)
  print("all good")
  print(s.find(0))
}
"#;

/// `strata test` compiles the tests with the JIT: the expected report, for
/// every example and for failing tests (fault, call stack, what they printed).
#[test]
fn the_jit_runs_tests_as_the_interpreter() {
    let strata = env!("CARGO_BIN_EXE_strata");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let report = |file: &Path, how: &str| output(Command::new(strata).arg("test").arg(how).arg("--file").arg(file));
    for ex in ["bank.strata", "builtins.strata", "math.strata", "orders.strata", "traits.strata", "tasks", "todos", "wc", "page"] {
        let f = root.join("examples").join(ex);
        let jit = report(&f, "--jit");
        assert!(!jit.0.contains("note: interpreted"), "{ex}: {}", jit.0);
        expected(&format!("tests-{}", ex.trim_end_matches(".strata")), &jit);
    }
    let d = tmp("jit-tests");
    let f = d.join("t.strata");
    save(&f, "fn half(n: Int) -> Int requires n % 2 == 0 { n / 2 }\nfn twice(n: Int) -> Int { half(n) * 4 }\ntest \"passes\" { assert twice(4) == 8 }\ntest \"fails\" { print(\"before\") print(twice(3)) }\ntest \"asserts\" { x := [1, 2] assert len(x) == 3 }\n");
    let jit = report(&f, "--jit");
    expected("tests-failing", &jit);
    assert!(jit.0.contains("✗ fails\n    Requires: precondition of `half` violated") && jit.0.contains("    stack: twice -> half\n    printed:\n      before\n") && jit.0.ends_with("1 passed, 2 failed\n"), "{}", jit.0);
}

/// `strata verify` fuzzes compiled (by default, as `--jit`): the same
/// verdicts as the interpreter's fuzzer (its inputs are the runtime's), a
/// runaway loop stopped by the step budget.
#[test]
fn verify_fuzzes_compiled() {
    let strata = env!("CARGO_BIN_EXE_strata");
    let d = tmp("jit-verify");
    let f = d.join("v.strata");
    save(&f, "fn pieces(s: String) -> Positive { len(split(s, \"a\")) - 1 }\nfn loops(n: Natural) -> Int { mut i := 0 while i != n * 2 + 1 { i += 2 } i }\nfn fine(xs: List[String]) -> Natural { len(join(xs, \",\").code_points()) }\n");
    let (out, _) = output(Command::new(strata).args(["verify", "--jit", "--file"]).arg(&f).env("STRATA_SOLVER", "none").env("STRATA_VERDICTS", d.join(".verdicts-jit")));
    assert!(!out.contains("note:"), "{out}");
    assert!(out.contains("result of `pieces`: 0 does not satisfy `Positive`") && out.contains("~ fine                     fuzzed: 200 cases, 0 discarded"), "{out}");
    let (out, _) = output(Command::new(strata).args(["verify", "--file"]).arg(&f).env("STRATA_SOLVER", "none").env("STRATA_VERDICTS", d.join(".verdicts")));
    assert!(!out.contains("note:"), "{out}");
    assert!(out.contains("✗ pieces                   Refinement violation (found by the fuzzer)") && out.contains("result of `pieces`: 0 does not satisfy `Positive`"), "{out}");
    assert!(out.contains("✗ loops                    Limit violation (found by the fuzzer)") && out.contains("too many steps (stack: loops)"), "{out}");
    assert!(out.contains("~ fine                     fuzzed: 200 cases, 0 discarded"), "{out}");
}

// No limit on recursion or loops: a recursion runs until the stack is
// nearly out, then faults (as compiled code does), its calls one entry of
// the stack line; a test's runaway recursion fails that test.
#[test]
fn recursion_runs_until_the_stack_is_out() {
    let d = tmp("no_limits");
    let strata = env!("CARGO_BIN_EXE_strata");
    let f = d.join("deep.strata");
    save(
        &f,
        r#"type Chain =
  | Link(Int, Chain)
  | End

fn down(n: Int) -> Int {
  if n == 0 { 0 } else { 1 + down(n - 1) }
}

fn length(c: Chain) -> Int {
  match c {
    Link(_, rest) => 1 + length(rest),
    End => 0,
  }
}

fn main() {
  print(down(20000))
  mut c: Chain = End
  mut i: Int = 0
  while i < 1500000 {
    i = i + 1
  }
  i = 0
  while i < 20000 {
    c = Link(i, c)
    i = i + 1
  }
  print(length(c))
  print(down(1000000000))
}

test "runaway" {
  assert down(100) == 100
  down(-1)
}
"#,
    );
    let o = Command::new(strata).args(["run"]).arg(&f).output().unwrap();
    let (out, err) = (String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert_eq!((out.as_ref(), o.status.code()), ("20000\n20000\n", Some(101)), "{err}");
    assert!(err.starts_with("fault (Limit): stack overflow (recursion too deep)\n    stack: main -> down ("), "{err}");
    assert!(err.ends_with(" times)\n") && err.lines().count() == 2, "{err}");
    let o = Command::new(strata).args(["test", "--file"]).arg(&f).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr);
    assert!(out.contains("✗ runaway\n    Limit: stack overflow (recursion too deep)\n    stack: down (") && out.contains("0 passed, 1 failed"), "{out}");

}

#[path = "common/json.rs"]
mod json_case;

/// `derive Json` and `std.json`'s text compiled: what the interpreter prints.
#[test]
fn json_compiles() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("derive-json");
    let (out, code) = case(&d, "json", json_case::PROGRAM);
    assert_eq!((out.as_str(), code), (json_case::OUTPUT, 0));
}

// `extern c`: the package's C (`[native]`), called from compiled code with
// each type converted both ways; the same output built and with the JIT.
#[test]
fn extern_c_calls() {
    if !have_clang() {
        return;
    }
    let d = tmp("extern_c");
    let strata = env!("CARGO_BIN_EXE_strata");
    std::fs::write(d.join("strata.toml"), "name = \"ffi\"\n\n[native]\nsources = [\"ffi.c\"]\n").unwrap();
    std::fs::write(
        d.join("ffi.c"),
        r#"#include <ctype.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
int8_t neg8(int8_t x) { return (int8_t)-x; }
uint8_t inc8(uint8_t x) { return (uint8_t)(x + 1); }
uint64_t umax(void) { return UINT64_MAX; }
int64_t imin(void) { return INT64_MIN; }
uint64_t half(uint64_t x) { return x / 2; }
bool even(int32_t x) { return x % 2 == 0; }
double scale(double x, bool twice) { return twice ? x * 2 : x; }
const char *shout(const char *s) {
  static char buf[64];
  size_t i = 0;
  for (; s[i] && i < 63; i++) buf[i] = (char)toupper((unsigned char)s[i]);
  buf[i] = 0;
  return buf;
}
typedef struct { int32_t n; } Counter;
int32_t counter_open(int32_t start, Counter **out) {
  if (start < 0) { *out = NULL; return 1; }
  *out = malloc(sizeof **out);
  (*out)->n = start;
  return 0;
}
int32_t counter_bump(Counter *c) { return ++c->n; }
void counter_close(Counter *c) { free(c); }
Counter *nothing(void) { return NULL; }
void twice(int32_t *x) { *x *= 2; }
"#,
    )
    .unwrap();
    let (ok, out) = (|| {
        let o = Command::new(strata).args(["--file", ".", "edit", "c"]).current_dir(&d).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let mut o = o;
        o.stdin.take().unwrap().write_all(
            br#"extern c type Counter

extern c fn neg8(x: I8) -> I8
extern c fn inc8(x: U8) -> U8
extern c fn umax() -> U64
extern c fn imin() -> I64
extern c fn half(x: U64) -> U64
extern c fn even(x: I32) -> Bool
extern c fn scale(x: Float, twice: Bool) -> Float
extern c fn shout(s: String) -> String
extern c fn counter_open(start: I32, mut c: Option[Counter]) -> I32
extern c fn counter_bump(c: Counter) -> I32
extern c fn counter_close(c: Counter)
extern c fn nothing() -> Counter
extern c fn twice(mut x: I32)

pub fn numbers() -> List[Int] {
  [neg8(5), neg8(-128), inc8(254), umax(), imin(), half(umax()), (if even(4) { 1 } else { 0 })]
}

pub fn texts() -> String {
  f"{shout("hello, c")} {scale(1.5, true)} {scale(1.5, false)}"
}

pub fn counting() -> List[Int] {
  mut c: Option[Counter] = None
  code := counter_open(41, mut c)
  mut x: I32 = 21
  twice(mut x)
  match c {
    Some(h) => {
      first := counter_bump(h)
      second := counter_bump(h)
      counter_close(h)
      [code, first, second, x]
    }
    None => [code],
  }
}

pub fn refused() -> List[Int] {
  mut c: Option[Counter] = None
  code := counter_open(-1, mut c)
  [code, (if c == None { 1 } else { 0 })]
}

pub fn null_handle() -> Int {
  counter_bump(nothing())
}

pub fn nul_string() -> String {
  shout("a" + chr(0) + "b")
}
"#,
        )
        .unwrap();
        let r = o.wait_with_output().unwrap();
        (r.status.success(), String::from_utf8_lossy(&r.stderr).to_string())
    })();
    assert!(ok, "{out}");
    save(
        &d.join("main.strata"),
        "use c\n\nfn main() {\n  print(c.numbers())\n  print(c.texts())\n  print(c.counting())\n  print(c.refused())\n}\n\ntest \"null\" {\n  c.null_handle()\n}\n\ntest \"nul\" {\n  c.nul_string()\n}\n",
    );
    // (beyond 64 bits, an `Int` shows as a string of digits)
    let want = "[-5, -128, 255, 18446744073709551615, -9223372036854775808, 9223372036854775807, 1]\nHELLO, C 3.0 1.5\n[0, 42, 43, 42]\n[1, 1]\n";
    // built: the same output, and every object freed
    let exe = d.join("prog");
    let b = Command::new(strata).args(["build", "-o"]).arg(&exe).current_dir(&d).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    let (out, code) = freed_everything(output(Command::new(&exe).current_dir(&d).env("STRATA_MEMORY", "check")), &d);
    assert_eq!((out.as_str(), code), (want, 0));
    {
        let (out, code) = output(Command::new(strata).args(["run", "--jit", "."]).current_dir(&d));
        assert_eq!((out.as_str(), code), (want, 0));
        // compiled tests: C's NULL for a handle, a NUL in a string
        let (out, _) = output(Command::new(strata).args(["test", "--jit"]).current_dir(&d));
        assert!(out.contains("`nothing` gave NULL for a `Counter` (declare it `Option[Counter]` if it can be NULL)"), "{out}");
        assert!(out.contains("a string passed to `shout` holds a NUL character"), "{out}");
    }
}

/// `for` over an `Iterator`, compiled (not compared with the interpreter,
/// which isn't changed for what compiled code handles: IDEAS.md), freeing
/// everything it makes.
#[test]
fn for_over_iterators_compiles() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("iterators");
    let f = d.join("iterators.strata");
    save(&f, json_case::ITERATORS);
    let strata = env!("CARGO_BIN_EXE_strata");
    let exe = d.join("prog");
    let b = Command::new(strata).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    let (out, code) = freed_everything(output(Command::new(&exe).current_dir(&d).env("STRATA_MEMORY", "check")), &f);
    assert_eq!((out.as_str(), code), (json_case::ITERATORS_OUTPUT, 0));
    // lazy adapters (closures in records, the prelude's impls), finite iterators
    let f = d.join("lazy.strata");
    save(&f, json_case::LAZY_ITERATORS);
    let b = Command::new(strata).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    let (out, code) = freed_everything(output(Command::new(&exe).current_dir(&d).env("STRATA_MEMORY", "check")), &f);
    assert_eq!((out.as_str(), code), (json_case::LAZY_ITERATORS_OUTPUT, 0));
    // comprehensions over iterators, collecting into sets and dicts
    let f = d.join("comps.strata");
    save(&f, json_case::ITERATOR_COMPREHENSIONS);
    let b = Command::new(strata).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    let (out, code) = freed_everything(output(Command::new(&exe).current_dir(&d).env("STRATA_MEMORY", "check")), &f);
    assert_eq!((out.as_str(), code), (json_case::ITERATOR_COMPREHENSIONS_OUTPUT, 0));
    // `iter()` on the built-in collections
    let f = d.join("collections.strata");
    save(&f, json_case::COLLECTION_ITERATORS);
    let b = Command::new(strata).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    let (out, code) = freed_everything(output(Command::new(&exe).current_dir(&d).env("STRATA_MEMORY", "check")), &f);
    assert_eq!((out.as_str(), code), (json_case::COLLECTION_ITERATORS_OUTPUT, 0));
    // a string's bytes, code points and graphemes (std.unicode, loaded for it)
    let f = d.join("units.strata");
    save(&f, json_case::STRING_UNITS);
    let b = Command::new(strata).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    let (out, code) = freed_everything(output(Command::new(&exe).current_dir(&d).env("STRATA_MEMORY", "check")), &f);
    assert_eq!((out.as_str(), code), (json_case::STRING_UNITS_OUTPUT, 0));
    // `len`, indexing and slicing on iterators
    let f = d.join("indexing.strata");
    save(&f, json_case::ITERATOR_INDEXING);
    let b = Command::new(strata).arg("build").arg(&f).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    let (out, code) = freed_everything(output(Command::new(&exe).current_dir(&d).env("STRATA_MEMORY", "check")), &f);
    assert_eq!((out.as_str(), code), (json_case::ITERATOR_INDEXING_OUTPUT, 0));
}

/// Types passed whole: compiled as interpreted.
#[test]
fn impl_type_parameters_compile() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("impl-statics");
    for (name, prog, want) in [("statics", json_case::STATICS, json_case::STATICS_OUTPUT), ("builtins", json_case::BUILTINS, json_case::BUILTINS_OUTPUT), ("text", json_case::TEXT, json_case::TEXT_OUTPUT), ("self_methods", json_case::SELF_METHODS, json_case::SELF_METHODS_OUTPUT), ("generic_derive", json_case::GENERIC_DERIVE, json_case::GENERIC_DERIVE_OUTPUT), ("option_option", json_case::OPTION_OPTION, json_case::OPTION_OPTION_OUTPUT), ("root_builtin_name", json_case::ROOT_BUILTIN_NAME, json_case::ROOT_BUILTIN_NAME_OUTPUT), ("refined", json_case::REFINED, json_case::REFINED_OUTPUT), ("refined_more", json_case::REFINED_MORE, json_case::REFINED_MORE_OUTPUT)] {
        let (out, code) = case(&d, name, prog);
        assert_eq!((out.as_str(), code), (want, 0));
    }
}

/// Values that stay in their block (`llvm/escape.rs`), next to values that
/// don't: those passed to a function that keeps them, copied, or returned.
const STACK_VALUES: &str = "type Name = { first: String, last: String }

type Person = { name: Name, age: Int, tags: List[String] }

// borrows `n`: a field read
fn full(n: Name) -> String {
    n.first + \" \" + n.last
}

// borrows `p` (passes it on to a function that borrows it)
fn label(p: Person) -> String {
    full(p.name) + \"/\" + to_string(len(p.tags))
}

// keeps `p`
fn keep(p: Person) -> List[Person] {
    [p]
}

// a value on the stack left by `return`
fn find(names: List[String], wanted: String) -> Int {
    mut i := 0
    while i < len(names) {
        n := Name { first: names[i], last: \"L\" }
        if n.first == wanted {
            return i
        }
        i += 1
    }
    -1
}

fn main() {
    mut total := 0
    mut kept := []
    mut i := 0
    while i < 1000 {
        i += 1
        n := Name { first: \"Ada\" + to_string(i), last: \"Lovelace\" }
        p := Person { name: Name { first: \"x\", last: to_string(i) }, age: i * 4611686018427387904, tags: [\"a\", to_string(i)] }
        if i % 3 == 0 {
            continue
        }
        total += byte_len(full(n)) + byte_len(label(p))
        if i == 500 {
            kept = kept + keep(p)
        }
        q := Person { name: n, age: i, tags: [] }
        match q {
            Person { age, .. } => {
                total += age % 5
            },
        }
        if i == 998 {
            break
        }
    }
    print(total)
    print(len(kept))
    names := [\"Ada1\", \"Ada7\", \"Bob\"]
    at := find(names, \"Ada7\")
    print(f\"found Ada7 at {at}\")
}
";

/// Fields assigned in place, and the values that must not change with them.
const IN_PLACE: &str = "type State = { n: Int, name: String, errs: List[String], at: (Int, String) }

type P = { x: Int, y: Int }

fn bump(mut s: State) {
    s.n += 1
    s.errs = push(s.errs, \"bumped\")
}

// replaces the whole value moved in
fn reset(mut s: State) {
    s = State { n: 42, name: \"reset\", errs: [\"r\"], at: (4, \"d\") }
}

// another argument mentions the variable: not moved
fn set_n(mut s: State, n: Int) {
    s.n = n
}

// passes it on
fn twice(mut s: State) {
    bump(mut s)
    bump(mut s)
}

fn both(a: State, b: Int) -> String {
    f\"{a.n} {b}\"
}

fn main() {
    mut s := State { n: 0, name: \"s\", errs: [], at: (0, \"a\") }
    mut i := 0
    while i < 1000 {
        s.n += 1
        s.name = \"s\" + to_string(i)
        s.errs = push(s.errs, to_string(i))
        i += 1
    }
    print(f\"{s.n} {s.name} {len(s.errs)}\")
    // a copy keeps its value
    t := s
    s.n = -1
    print(f\"{t.n} {s.n}\")
    // so does the value in a list
    xs := [s]
    s.name = \"changed\"
    print(f\"{xs[0].name} {s.name}\")
    // an operand that reads `s` before a later one assigns it
    print(both(s, {
        s.n = 7
        2
    }))
    // a loop over a field the body assigns
    mut k := 0
    for e in s.errs {
        if k < 3 {
            s.errs = push(s.errs, e)
        }
        k += 1
    }
    print(f\"{k} {len(s.errs)}\")
    // a `mut` parameter
    bump(mut s)
    print(f\"{s.n} {s.errs[len(s.errs) - 1]}\")
    twice(mut s)
    set_n(mut s, s.n + 1)
    print(f\"{s.n} {len(s.errs)}\")
    u := s
    reset(mut s)
    print(f\"{s.n} {s.name} {u.n} {u.name}\")
    // a task holding `s` while it changes
    r := scope {
        h := spawn s.n * 10
        s.n = 100
        await h
    }
    print(f\"{r} {s.n}\")
    // a tuple field, and a tuple
    s.at = (1, \"b\")
    mut pair := (1, \"one\")
    pair.1 = \"uno\"
    print(f\"{s.at.1} {pair.1}\")
    // a constant record (every field a literal): never written in place
    mut c := P { x: 1, y: 2 }
    c.x = 10
    d := P { x: 1, y: 2 }
    print(f\"{c.x} {d.x}\")
    print(\"done\")
}
";

/// Dictionaries and sets updated by `x = insert(x, …)`, and the values that
/// must not change with them.
const INSERT_IN_PLACE: &str = "type Entry = { name: String, n: Int }

type State = { seen: Set[Int], names: Dict[String, Entry], count: Int }

fn note(mut s: State, k: String, n: Int) {
    s.names = insert(s.names, k, Entry { name: k + \"!\", n })
    s.seen = s.seen.insert(n)
    s.count += 1
}

fn main() {
    mut d := Dict[String, Int] {}
    mut i := 0
    while i < 2000 {
        d = insert(d, \"k\" + to_string(i % 700), i)
        i += 1
    }
    (k5, k699) := (\"k5\", \"k699\")
    print(f\"{len(d)} {get(d, k5) ?? -9} {get(d, k699) ?? -9}\")
    // a copy keeps its value
    old := d
    d = insert(d, \"new\", -1)
    d = insert(d, \"k5\", 5)
    new := \"new\"
    print(f\"{len(old)} {len(d)} {get(old, k5) ?? -9} {get(d, k5) ?? -9} {contains(old, new)}\")
    // so does one in a list
    ds := [d]
    d = insert(d, \"later\", 0)
    print(f\"{len(ds[0])} {len(d)}\")
    // a constant dictionary
    mut c := Dict[String, Int] { \"a\": 1, \"b\": 2 }
    c = insert(c, \"c\", 3)
    c = insert(c, \"a\", 10)
    e := Dict[String, Int] { \"a\": 1, \"b\": 2 }
    print(f\"{c} {e}\")
    // a value mentioning the dictionary itself
    mut dd := Dict[String, Int] { \"x\": 1 }
    dd = insert(dd, \"size\", len(dd))
    dd = insert(dd, \"size2\", len(dd))
    print(dd)
    // sets, through a variable and a method call
    mut seen := Set[Int] {}
    for n in [5, 3, 9, 3, 1, 5, 7] {
        seen = insert(seen, n)
        seen = seen.insert(n * 100)
    }
    print(seen)
    // fields of a record, through a `mut` parameter
    mut s := State { seen: Set[Int] {}, names: Dict[String, Entry] {}, count: 0 }
    for w in [\"b\", \"a\", \"c\", \"a\"] {
        note(mut s, w, len(w.bytes()) + s.count)
    }
    t := s
    note(mut s, \"z\", 99)
    a := get(s.names, \"a\") ?? Entry { name: \"?\", n: -9 }
    print(f\"{s.count} {len(s.names)} {a.name} {a.n} {s.seen} {t.count} {len(t.names)}\")
    print(\"done\")
}
";

/// Constructors as arguments of functions that only read them (on the
/// stack) or keep them, and removals from dictionaries and sets.
const MORE_IN_PLACE: &str = "type P = { x: Int, name: String }

type Box = { p: P, tag: String }

fn norm(p: P) -> Int {
    p.x * 2 + byte_len(p.name)
}

fn wrap(p: P) -> Box {
    Box { p, tag: \"w\" }
}

fn first(a: P, b: P) -> Int {
    a.x + b.x
}

fn main() {
    // constructors as arguments: on the stack when the callee only reads them
    mut total := 0
    mut kept := []
    mut i := 0
    while i < 1000 {
        total += norm(P { x: i, name: \"n\" + to_string(i) })
        // a variable holding the call's result, not the argument
        c := norm(P { x: i, name: \"c\" })
        total += c
        total += first(P { x: 1, name: to_string(i) }, P { x: i, name: \"b\" })
        if i % 100 == 0 {
            kept = push(kept, wrap(P { x: i, name: \"kept\" + to_string(i) }))
        }
        i += 1
    }
    print(f\"{total} {len(kept)} {kept[3].p.name}\")
    // removing in place
    mut d := Dict[String, Int] {}
    for k in 0..300 {
        d = insert(d, \"k\" + to_string(k), k)
    }
    old := d
    for k in 0..300 {
        if k % 3 == 0 {
            d = remove(d, \"k\" + to_string(k))
        }
    }
    d = remove(d, \"absent\")
    print(f\"{len(d)} {len(old)}\")
    mut s := Set[Int] { 1, 2, 3 }
    s = s.remove(2)
    s = remove(s, 9)
    t := s
    s = remove(s, 1)
    print(f\"{s} {t}\")
    mut c := Dict[String, Int] { \"a\": 1, \"b\": 2 }
    c = remove(c, \"a\")
    e := Dict[String, Int] { \"a\": 1, \"b\": 2 }
    print(f\"{c} {e}\")
    print(\"done\")
}
";
