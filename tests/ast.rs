//! `std.ast.binary` reads module files and bundles as `src/binary.rs` does,
//! and writes a definition's bytes as it does. For every module file in the
//! repository, and for a program using every node of the tree, the Strata
//! decoder's trees (as `debug` prints them) are `strata header --tree`'s, and
//! each definition it re-encodes is the file's bytes. Cut or corrupted files
//! are errors, never crashes.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const STRATA: &str = env!("CARGO_BIN_EXE_strata");

/// The checker, run as `MODE FILE...`: `tree` prints each definition as
/// `debug` writes it; `same` prints `N/M` (of M definitions, N re-encode to
/// the file's bytes) and the names of those that don't; `bad` reads every
/// prefix of the file and the file with each byte flipped, and prints how many
/// prefixes were read (none should be).
const CHECKER: &str = r#"use std.fs.{Files, Disk}
use std.process.{Environment, Process}
use std.ast.binary.{read, read_header, encode_definition}

fn tree(bytes: List[Int]) -> Result[List[String], String] {
  Ok(read(bytes)!.decls.map(d -> debug(d)))
}

fn same(bytes: List[Int]) -> Result[List[String], String] {
  decls := read(bytes)!.decls
  h := read_header(bytes)!
  mut n := 0
  mut wrong := []
  for p in zip(h.entries, decls) {
    if encode_definition(p.1) == slice(bytes, p.0.offset, p.0.offset + p.0.size) {
      n += 1
    } else {
      wrong = push(wrong, p.0.name)
    }
  }
  m := len(decls)
  Ok(push(wrong, f"{n}/{m}"))
}

fn bad(bytes: List[Int]) -> Result[List[String], String] {
  mut read_cut := 0
  for n in 0..len(bytes) {
    if read(slice(bytes, 0, n)) is Ok {
      read_cut += 1
    }
  }
  mut read_flipped := 0
  for i in 0..len(bytes) {
    flipped := [if j == i {
      255 - b
    } else {
      b
    } for (j, b) in enumerate(bytes)]
    if read(flipped) is Ok {
      read_flipped += 1
    }
  }
  Ok([f"cut {read_cut}", f"flipped {read_flipped}"])
}

fn main(disk: Disk, process: Process) {
  args := process.args()
  mode := get(args, 0) ?? ""
  for path in drop(args, 1) {
    bytes := match disk.read_bytes(path) {
      Ok(bs) => bs,
      Err(_) => [],
    }
    out := match mode {
      "tree" => tree(bytes),
      "same" => same(bytes),
      _ => bad(bytes),
    }
    match out {
      Ok(lines) => {
        for l in lines {
          print(l)
        }
      },
      Err(e) => print(f"error: {e}"),
    }
  }
}
"#;

/// A program with every node of the tree (it parses; it needn't check).
const EVERY_NODE: &str = r#"/// doc
// comment
pub distinct type Id = Int where it > 0

type R[T] = {
  /// f
  a: T,
  b: (Int, String) -> (),
  c: Int = 3,
}

type S =
  | A
  | B(Int, Float)
  | C { x: Int | String, y: Int where it < 5 }

mock M = { n: Int }

derivable(Record) trait Named {
  fn name() -> String
}

trait Tr[P]: Eq {
  fn m(self: Self, mut q: P) -> Int
  fn d(self) -> Int {
    1
  }
  law l(a: Self): a == a
}

impl[T: Eq + Tr[Int]] Tr[Int] for R[T] {
  fn m(self: Self, mut q: Int) -> Int {
    0
  }
}

impl S {
  fn size(self) -> Int {
    0
  }
}

derive Named for R

comptime fn twice(comptime x: Int) -> Int {
  x * 2
}

extern c type Handle

extern c fn strlen(s: String) -> U64

pub capability trait Ticks {
  fn tick(mut self) -> Int
}

capability Clock {
  fn now() -> Int {
    0
  }
}

fn f(x: Int) -> Int
  requires x > 0
  ensures it >= 0
  terminates
{
  (a, _) := (1, 2.5)
  mut v: Int = -3
  neg := -x
  big := 99999999999999999999999
  e := [1000000000000000000000.0, -0.0, 0.1, 0.00000015, 123456789.125, 0.5, -0.0]
  v = v + 1
  while v < 0 {
    v = v + 1
  }
  for [h, ..t] in [[1]] {
    break
  }
  loop {
    continue
  }
  assert !true ==> false
  // c
  g := (y: Int) -> y % 2
  h := (y) -> y / 2 - 1 * 3
  z := match S.A as s {
    // arm
    A | B(_, _) => 1,
    C { x: 3, .. } => 2,
    B("s", true) => 4,
    where it is C => 3,
    _ => ?hole,
  }
  o := Some(v)? + Ok(1)! + (Err(2) except e { e }) + (Err(3) !! 4)
  q := None ?? 5
  r := R { a: 1, b: g, c: 2 }
  r2 := R { ..r, a: 2 }
  print(f"{x:>08.2} and {r.a} {x:*^+5} {x}")
  mm := mock Tr
  ys := [n * 2 for n in 0..10 where n % 2 == 0]
  st := Set { n for n in 1..=3 }
  dc := Dict { n: n for n in ys[..3] }
  d := Dict[String, Int] { "a": 1 }
  se := Set[Int] { 1, 2 }
  ix := ys[1] + ys[1..][0]
  ok := x < 1 || x <= 2 && x > 3 && x >= 4 && x != 5
  t := comptime twice(2)
  desc := $Self
  comptime for fld in $Self.fields {
    print(fld)
  }
  w := match comptime t {
    List[Int] => 1,
    Option[k] => 2,
    v => 3 for v in $Self.variants,
  }
  if "s".len() == 1 {
    return 1
  } else {
    f(mut v)
  }
}

test "t"(clock: Clock) {
  assert f(1) == 1
}

use a.b.{c, d}
use e.*
"#;

/// What `strata header --tree` of the program above must show: a variant
/// or a field value of each kind.
const NODES: &[&str] = &["TypeDef", "FnDef", "TestDef", "TraitDef", "ImplDef", "UseDef", "ExternDef", "CapabilityDef", "Alias", "RecordBody", "SumBody", "NoPayload", "Positional", "RecordPayload", "ExternFn", "ExternType", "Named", "FnType", "UnitType", "Refined", "UnionType", "IntLit", "BigIntLit", "FloatLit", "TupleExpr", "StrLit", "BoolLit", "UnitExpr", "Var(", "Hole(", "ListExpr", "CtorExpr", "Call {", "MethodCall", "FieldOf", "Lambda", "Binary", "Unary", "IsExpr", "IfExpr", "MatchExpr", "MutExpr", "Block {", "MockExpr", "Return(", "Try {", "Except {", "Interp(", "Range {", "Index {", "Comp {", "DictExpr", "SetExpr", "Fallback", "Comptime(", "Describe(", "NoArgs", "PositionalArgs", "RecordArgs", "UpdateArgs", "ListComp", "SetComp", "DictComp", "TextPart", "ValuePart", "Some(FmtSpec", "op: Add", "op: Sub", "op: Mul", "op: Div", "op: Mod", "op: Equal", "op: NotEqual", "op: Less,", "op: LessEq", "op: Greater,", "op: GreaterEq", "op: And", "op: Or", "op: Implies", "op: Neg", "op: Not", "TryResult", "TryOption", "Let {", "VarDecl", "Assign {", "While {", "For {", "Loop(", "Break", "Continue", "ExprStmt", "Assert(", "Comment(", "ComptimeFor", "Wild", "Bind(", "IntPat", "StrPat", "BoolPat", "CtorPattern", "ListPat", "TuplePat", "CondPat", "OrPat", "ForPat", "NoPats", "PositionalPats", "RecordPats", "TypePats", "mocked: true", "Some(Derivable", "compile_time: true", "distinct: true", "e+21", "FloatLit(-0.0)", "e-7", "align: Some", "precision: Some", "items: None", "binding: Some", "rest: Some", "capability: true", "derived: true", "terminating: true", "mutable: true", "default: Some", "inclusive: true", "lo: None"];

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("strata-ast-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Store `text` as the module file `file`.
fn save(file: &Path, text: &str) {
    let mut c = Command::new(STRATA).args(["--file", &file.to_string_lossy(), "edit"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(file.exists(), "{}", String::from_utf8_lossy(&o.stderr));
}

fn run(args: &[&str]) -> String {
    let o = Command::new(STRATA).args(args).output().unwrap();
    assert!(o.status.success(), "strata {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}

/// The checker, stored once per test.
fn checker(dir: &Path) -> String {
    let file = dir.join("check.strata");
    save(&file, CHECKER);
    file.to_string_lossy().into_owned()
}

/// Every module file in the repository: std, the examples, test data and the
/// compiled prelude.
fn module_files() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = vec![];
    let mut todo = vec![root.join("std"), root.join("examples"), root.join("tests/data"), root.join("src/llvm")];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                todo.push(p);
            } else if p.extension().is_some_and(|x| x == "strata") && std::fs::read(&p).unwrap().starts_with(b"STRATA\x01\x00") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// The Strata decoder agrees with Rust's on `files`: the same trees, and
/// each definition re-encoded to the same bytes.
fn agree(check: &str, files: &[PathBuf]) {
    let paths: Vec<String> = files.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let mut args = vec!["--file", check, "run", "--", "tree"];
    args.extend(paths.iter().map(|s| s.as_str()));
    let got = run(&args);
    let want: String = paths.iter().map(|p| run(&["header", p, "--tree"])).collect();
    if got != want {
        let (g, w): (Vec<&str>, Vec<&str>) = (got.lines().collect(), want.lines().collect());
        let at = g.iter().zip(&w).position(|(a, b)| a != b).unwrap_or(g.len().min(w.len()));
        panic!("definition {at} differs:\n  std.ast: {}\n  Rust:    {}", g.get(at).unwrap_or(&"(none)"), w.get(at).unwrap_or(&"(none)"));
    }
    args[4] = "same";
    for (p, line) in paths.iter().zip(run(&args).lines()) {
        let (n, m) = line.split_once('/').unwrap_or_else(|| panic!("{p}: {line}"));
        assert_eq!(n, m, "{p}: only {line} definitions re-encode to the file's bytes");
    }
}

#[test]
fn every_module_file_reads_and_writes_as_in_rust() {
    let dir = scratch("files");
    let files = module_files();
    assert!(files.len() > 30, "{files:?}");
    agree(&checker(&dir), &files);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_node_reads_and_writes_as_in_rust() {
    let dir = scratch("nodes");
    let file = dir.join("every.strata");
    save(&file, EVERY_NODE);
    let tree = run(&["header", &file.to_string_lossy(), "--tree"]);
    for n in NODES {
        assert!(tree.contains(n), "the program has no {n}");
    }
    agree(&checker(&dir), &[file]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bundles_read_as_in_rust() {
    let dir = scratch("bundle");
    let bundle = dir.join("tasks.strata");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    run(&["--file", &root.join("examples/tasks").to_string_lossy(), "pack", &bundle.to_string_lossy()]);
    let check = checker(&dir);
    let b = bundle.to_string_lossy();
    let got = run(&["--file", &check, "run", "--", "tree", &b]);
    assert!(got.lines().count() > 10, "{got}");
    assert_eq!(got, run(&["header", &b, "--tree"]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_input_is_an_error_not_a_crash() {
    let dir = scratch("bad");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let check = checker(&dir);
    let file = root.join("examples/orders.strata");
    let out = run(&["--file", &check, "run", "--", "bad", &file.to_string_lossy()]);
    // every prefix is refused; a flipped byte may still decode (no hash check
    // yet), but never crashes
    assert!(out.starts_with("cut 0\nflipped "), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}
