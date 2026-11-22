//! `compiler/check`, `compiler/exhaustive` and `compiler/contracts` (the type checker in Strata:
//! the compiler package's `check` command) find the errors `strata check`
//! finds (clashing definitions, unknown names, wrong numbers of arguments,
//! type mismatches, non-exhaustive matches, contracts and refinements), worded
//! as it words them.

use std::io::Write;
use std::process::{Command, Stdio};

#[path = "common/compiler.rs"]
mod compiler;

const STRATA: &str = env!("CARGO_BIN_EXE_strata");

const CLASHES: &str = r#"type Line = { a: Int }
type E =
  | Line(Int)
  | Y
type Option = { b: Int }
fn main() { print(1) }
"#;

const ARITY: &str = r#"type Pt = { x: Int, y: Int }
type Sh =
  | Sq(Int)
  | Rc(Int, Int)
fn f(a: Int, b: Int) -> Int { a + b }
fn g(x: Int) -> Int { f(f(x)) + zz }
fn h() -> Sh { Rc(f(1), yy, 3) }
fn m() -> Sh { Sq }
fn n() -> Pt { Pt(1, 2) }
fn o(f: Int) -> Int { f }
fn main() { print(g(1)) }
"#;

const TYPES: &str = r#"fn f(a: Int) -> Int { a + "x" }
fn g(a: Int) -> String { a }
fn h(s: String) -> Int { f(s) }
fn k() -> Bool { 1 < 2.5 }
fn m() -> Int { x := true
 x + 1 }
fn n() -> List[Int] { [1, "a"] }
fn o(b: Bool) -> Int { if b { 1 } else { "no" } }
fn p() -> Float { 1 }
fn main() { print(1) }
"#;

const STATEMENTS: &str = r#"fn g(x: Int) -> Int { x }
fn a() { g(1) }
fn b(c: Bool) -> Int { if c { 1 } }
fn d(c: Bool) { if c { g(1) } }
fn e() -> Int {
  mut s := 0
  s = "x"
  while 1 { s += 1 }
  assert 2
  -true
  !3
  return "r"
}
fn h(c: Bool) -> Int { match c { true => 1, false => "f" } }
fn k(c: Bool) -> Int {
  let y: String = 3
  if c { [1] } else { "s" }
  0
}
fn l(n: Int) -> Int {
  mut i := 0
  while i < n
    invariant i {
    i += 1
  }
  i
}
fn main() { print(1) }
"#;

const BUILTINS: &str = r#"fn a() -> String { abs(3) }
fn b() -> Int { abs("x") }
fn c(xs: List[Int]) -> String { len(xs) }
fn d(xs: List[Int]) -> Int { push(xs, "s") }
fn e(s: String) -> Int { split(s, ",") }
fn f(xs: List[String]) -> String { join(xs, 3) }
fn g() -> Int { sqrt(2.0) }
fn h(xs: List[Int]) -> Int { xs.map(x -> x + 1) }
fn k(s: String) -> Bool { s.starts_with(1) }
fn m(xs: List[Int]) -> String { sum(xs) }
fn main() { print(1) }
"#;

const TUPLES: &str = r#"fn a() -> (Int, String) { (1, 2) }
fn b(p: (Int, String)) -> Int { p.1 }
fn c(p: (Int, String)) -> String { (n, s) := p
  n }
fn d(xs: List[Int]) -> String {
  mut out := ""
  for x in xs { out = out + x }
  out
}
fn e(xs: List[(Int, String)]) -> Int {
  mut t := 0
  for (n, s) in xs { t = t + s }
  t
}
fn f(xs: List[String]) -> Int {
  mut t := 0
  for (i, s) in enumerate(xs) { t = t + s }
  t
}
fn g(p: (Int, Int)) -> Int { p.2 }
fn h() -> List[Int] { [x * 2 for x in ["a"]] }
fn main() { print(1) }
"#;

const RESULTS: &str = r#"fn p(s: String) -> Result[Int, String] { Ok(1) }
fn a(s: String) -> Result[String, String] { n := p(s)!
  Ok(n) }
fn b(s: String) -> String { p(s) except e { "x" } }
fn c(s: String) -> Int { p(s) except { "bad" } }
fn d(xs: List[Int]) -> String { xs[0] }
fn e(xs: List[Int]) -> Int { xs[1..2] }
fn f() -> String { 0..3 }
fn g(xs: List[Int]) -> Int { xs["a"] }
fn h(o: Option[Int]) -> Option[String] { n := o?
  Some(n) }
fn main() { print(1) }
"#;

const NARROWED: &str = r#"type Sh =
  | Sq(Int)
  | Rc { w: Int, h: Int }
  | Dot
fn take(s: Sh) -> Int { 0 }
fn a(s: Sh) -> Int { match s { Sq(_) => s, _ => 0 } }
fn b(s: Sh) -> String { match s { Rc { .. } => s.w, _ => "z" } }
fn c(s: Sh) -> Int { match s { Sq(_) => take(s), _ => 0 } }
fn d(s: Sh) -> Bool { match s { Sq(_) => s == Dot, _ => true } }
fn e(s: Sh) -> Int { match s { Sq(_) | Dot => s, _ => 0 } }
fn f(s: Sh) -> Int { match s { Rc { .. } => s.z, _ => 0 } }
fn h(s: Sh) -> Int {
  if s is Sq { return s }
  0
}
fn main() { print(1) }
"#;

const CONTRACTS: &str = r#"type Pos = Int where it > 0
type Bad = Int where it + 1
type Name = String where it != "" && len(it.code_points()) <= 3
type Pt = { x: Int where it >= 0 = -1, y: Int where 1 < it = 2, n: Name = "abcd" }
type Sh =
  | Sq { side: Pos = 0 }
  | Dot
fn f(a: Int where it > 0) -> Int where it > 1
  requires 1
  requires it > 0
  ensures "s"
  ensures it > a
{ a + 1 }
fn g(a: Int where 3, b: Float) -> Pos
  requires b * 2.0
  ensures it
{ 1 }
fn h(mut n: Name) -> Int {
  mut p: Pos = 1
  p = 0
  p = -3
  mut q: Int where it < 10 || it == 20 = 1
  q = 20
  q = 15
  n = ""
  n = "ok"
  if q > 2 { p = 0 }
  q
}
trait Sz {
  fn size(self) -> Int requires 2 ensures 1.5
  law nonneg(x: Self): 3
  law ok(x: Self): x.size() >= 0
}
impl Sz for Pt {
  fn size(self) -> Int requires 7 ensures it % 2 { 1 }
}
test "t" (n: Int, xs: List[Int]) { assert true }
fn main() { print(1) }
"#;

const MUTS: &str = r#"type Pt = { x: Int, y: Int }
fn bump(mut n: Int) { n += 1 }
fn show(n: Int) -> Int { n }
fn shift(mut p: Pt) { p.x = 3 }
fn a() {
  x := 1
  x = 2
  mut y := 1
  y = 3
  bump(y)
  bump(mut y)
  show(mut y)
  bump(mut x)
  bump(3)
  q := mut y
  p := Pt { x: 1, y: 2 }
  p.x = 4
  shift(mut p)
  mut r := Pt { x: 1, y: 2 }
  shift(r)
  f := k -> { y = k }
  f(mut y)
  for i in [1, 2] { i = 3 }
  if true {
    y := 5
    y = 6
  }
  y = 7
}
fn b(n: Int, mut m: Int) {
  n = 1
  m = 2
  bump(mut n)
}
fn c(mut n: Int) {
  n := 1
  n = 2
}
fn main() { print(1) }
"#;

const TRAITS: &str = r#"trait Area {
  fn area(self) -> Int
}
type Sq = { s: Int }
type Pt = { x: Int }
impl Area for Sq {
  fn area(self) -> Int { self.s * self.s }
}
type Box[T] = { item: T }
impl[T] Box[T] {
  fn get(self) -> T { self.item }
  fn bad(self) -> Int { self.item }
}
fn total[T: Area](xs: List[T]) -> Int { 0 }
fn a(q: Sq) -> String { q.area() }
fn b(p: Pt) -> Int { total([p]) }
fn c(b: Box[Int]) -> String { b.get() }
fn d[T: Area](x: T) -> String { x.area() }
fn main() { print(1) }
"#;

const UNIONS: &str = r#"type Id = Int | String
type Small = Int where it < 10
fn a(n: Natural) -> String { n }
fn b(n: Natural) -> Int { n + 1 }
fn c(x: Int | String) -> Bool { x }
fn d() -> Id { 2.5 }
fn e() -> Id { 3 }
fn f(s: Small) -> String { s }
fn g(n: Int) -> Natural { n }
fn h() -> Natural { -1 }
fn k(x: Id) -> Int { x }
fn a2(x: Bool | String | Int) -> Float { x }
fn b2(c: Bool) -> Float { if c { "s" } else { 1 } }
fn d2() -> Float { let y: Id = 3
  y }
fn e2(c: Bool) -> Float { z := if c { "s" } else { 1 }
  z }
fn main() { print(1) }
"#;

const STATIC: &str = r#"type Item = { n: Int }
impl Item {
  fn new(n: Int) -> Item { Item { n } }
  fn size(self) -> Int { self.n }
}
trait Zero {
  fn zero() -> Self
}
impl Zero for Item {
  fn zero() -> Self { Item { n: 0 } }
}
fn a() -> Int { Item.new(1) }
fn b() -> Item { Item.new("x") }
fn c() -> Item { Item.make() }
fn d[T: Zero]() -> Int { T.zero() }
fn e(i: Item) -> Int { i.nope() }
fn f() -> String { Item.zero() }
fn main() { print(1) }
"#;

const UNION_ARMS: &str = r#"fn a(x: Int | String) -> String { if x is Int { x } else { x } }
fn b(x: Int | String) -> Bool {
  if x is String { return true }
  x
}
fn c(x: Int | String) -> Bool { match x { Int => x, String => false } }
fn main() { print(1) }
"#;

const WHOLE_ARMS: &str = r#"type Sh =
  | Sq(Int)
  | Rc(Int, Int)
  | Dot
fn a(x: Int | String) -> Bool { match x { Int => true, other => other } }
fn b(s: Sh) -> Int { match s { Dot => 0, other => other } }
fn c(s: Sh) -> Int { match s { Dot => 0, Sq(_) => 1, other => other } }
fn d(o: Option[Int]) -> String { match o { None => "n", other => other } }
fn e(mut s: Sh) -> Int { match s { Sq(_) => {
  s = Dot
  s
}, _ => 0 } }
fn main() { print(1) }
"#;

const RECORDS: &str = r#"type Pt = { x: Int, y: Int }
type Sh =
  | Sq(Int)
  | Rc { w: Int, h: Int }
  | Dot
fn a() -> Pt { Pt { x: 1, y: "two" } }
fn b(p: Pt) -> String { p.x }
fn c() -> Sh { Sq("s") }
fn d() -> Sh { Rc { w: true, h: 2 } }
fn e(s: Sh) -> Int { match s { Sq(n) => n, Rc { w, h } => w + h, Dot => "z" } }
fn f() -> Int { Dot }
fn g(p: Pt) -> Pt { Pt { ..p, x: "q" } }
fn h(p: Pt) -> Bool { p.z }
fn k(s: Sh) -> String { match s { Sq(n) => n, _ => "o" } }
fn m(p: Pt) -> Int { p == Dot }
fn main() { print(1) }
"#;

const GENERICS: &str = r#"type Box[T] = { item: T }
fn id[T](x: T) -> T { x }
fn first[T](xs: List[T], d: T) -> T { d }
fn a() -> String { id(1) }
fn b() -> Int { first([1, 2], "z") }
fn c[T](x: T) -> Int { x }
fn d() -> Box[Int] { Box { item: "s" } }
fn e(b: Box[String]) -> Int { b.item }
fn f() -> Option[Int] { Some("x") }
fn g() -> Result[Int, String] { Err(3) }
fn mk() -> Option[Int] { None }
fn h() -> Int { mk() ?? "d" }
fn k() -> Option[String] { Some(id(2)) }
fn m() -> Option[Int] { 3 }
fn main() { print(1) }
"#;

const METHODS: &str = r#"type Pt = { x: Int, y: Int }
impl Pt {
  fn norm(self) -> Int { self.x + self.y }
  fn scale(self, k: Int) -> Pt { Pt { x: self.x * k, y: self.y * k } }
  fn bad(self) -> String { self.x }
}
fn a(p: Pt) -> String { p.norm() }
fn b(p: Pt) -> Pt { p.scale("2") }
fn c(p: Pt) -> Int { p.scale(2, 3).norm() }
fn e(p: Pt) -> Int { p.scale(2).x }
fn main() { print(1) }
"#;

const TRAIT_METHODS: &str = r#"type Pt = { x: Int, y: Int }
trait Area {
  fn area(self) -> Int
}
impl Area for Pt {
  fn area(self) -> Int { "a" }
}
impl Pt {
  fn me(self) -> Self { 3 }
}
fn twice(p: Pt, k: Int) -> Int { k }
fn a(p: Pt) -> String { p.area() }
fn b(p: Pt) -> Int { p.twice("x") }
fn main() { print(1) }
"#;

const SUMS: &str = r#"type Sh =
  | Sq(Int)
  | Rc(Int, Int)
  | Dot
fn a(s: Sh) -> Int { s }
fn e(o: Option[Int]) -> String { o }
fn f(o: Option[Int]) -> String { match o { Some(n) => n, None => "z" } }
fn g(o: Option[Int]) -> String { o ?? 1 }
fn h(s: Sh) -> Bool { s == 3 }
fn main() { print(1) }
"#;

const UNKNOWN: &str = r#"type Pt = { x: Int, y: Int }
fn f(a: Int) -> Int {
  b := a + c
  mut s := 0
  s += b
  if b > 0 { d } else { a + s }
}
fn g(xs: List[Int]) -> Int {
  ys := xs.map(x -> x + y)
  for z in xs { print(z + w) }
  zs := [u * 2 for u in xs where u > v]
  p := Pt { x: 1, y: r }
  print(f"{p.x} {nowhere}")
  match ys { [h, ..rest] => h + len(rest) + k, _ => q }
}
fn l(n: Int) -> Int {
  mut i := 0
  while i < n
    invariant i <= n + m {
    i += 1
  }
  i
}
fn main() { print(f(1) + g([1]) + nope(2) + l(1)) }
"#;

const MATCHES: &str = r#"type Sh =
  | Sq(Int)
  | Rc { w: Int, h: Int }
  | Dot
fn a(s: Sh) -> Int { match s { Sq(n) => n, Dot => 0 } }
fn b(o: Option[Int]) -> Int { match o { Some(n) => n } }
fn c(r: Result[Int, String]) -> Int { match r { Err(_) => 0 } }
fn d(x: Bool) -> Int { match x { true => 1 } }
fn e(s: Sh) -> Int { match s { Rc { w, h } => w, Sq(_) | Dot => 0 } }
fn g(o: Option[Sh]) -> Int { match o { None => 1 } }
fn h(s: Sh) -> Int { match s { Sq(n) => n, _ => 0 } }
fn mk() -> Option[Sh] { None }
fn m() -> Int { match mk() { None => 0 } }
fn main() { print(1) }
"#;

fn save(file: &std::path::Path, text: &str) {
    let mut c = Command::new(STRATA).args(["--file", &file.to_string_lossy(), "edit"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    c.wait_with_output().unwrap();
}

#[test]
fn clashes_as_in_rust() {
    same_errors("clashes", CLASHES, &["defined twice", "already defined in the prelude", "two `impl", "already implemented", "a variant is a type of its own"]);
}

#[test]
fn unknown_names_as_in_rust() {
    same_errors("unknown", UNKNOWN, &["unknown variable"]);
}

#[test]
fn arity_as_in_rust() {
    same_errors("arity", ARITY, &["unknown variable", "expects", "needs"]);
}

#[test]
fn types_as_in_rust() {
    same_errors("types", TYPES, &["expected `", "Int or Float"]);
}

#[test]
fn statement_types_as_in_rust() {
    same_errors("statements", STATEMENTS, &["expected `", "Int or Float"]);
}

#[test]
fn record_and_sum_types_as_in_rust() {
    same_errors("records", RECORDS, &["expected `", "Int or Float", "has no field", "missing field"]);
}

#[test]
fn generics_as_in_rust() {
    same_errors("generics", GENERICS, &["expected `", "handle an Option"]);
}

#[test]
fn non_exhaustive_matches_as_in_rust() {
    same_errors("matches", MATCHES, &["non-exhaustive match"]);
}

#[test]
fn methods_as_in_rust() {
    same_errors("methods", METHODS, &["expected `", "expects"]);
}

#[test]
fn trait_methods_as_in_rust() {
    same_errors("traits", TRAIT_METHODS, &["expected `", "expects"]);
}

#[test]
fn sum_variables_as_in_rust() {
    same_errors("sums", SUMS, &["expected `"]);
}

#[test]
fn builtin_calls_as_in_rust() {
    same_errors("builtins", BUILTINS, &["expected `", "no version of"]);
}

#[test]
fn tuples_and_loops_as_in_rust() {
    same_errors("tuples", TUPLES, &["expected `", "Int or Float", "no element"]);
}

#[test]
fn results_and_indexes_as_in_rust() {
    same_errors("results", RESULTS, &["expected `"]);
}

#[test]
fn narrowed_variables_as_in_rust() {
    same_errors("narrowed", NARROWED, &["expected `", "has no field"]);
}

#[test]
fn bounds_and_generic_impls_as_in_rust() {
    same_errors("bounds", TRAITS, &["expected `", "does not implement"]);
}

#[test]
fn mut_rules_as_in_rust() {
    same_errors("muts", MUTS, &["mut", "closure"]);
}

#[test]
fn unions_and_refinements_as_in_rust() {
    same_errors("unions", UNIONS, &["expected `"]);
}

#[test]
fn static_calls_and_unknown_methods_as_in_rust() {
    same_errors("static", STATIC, &["expected `", "has no method", "unknown function", "needs"]);
}

#[test]
fn contracts_and_refinements_as_in_rust() {
    same_errors("contracts", CONTRACTS, &["clause", "refinement", "law `", "does not satisfy", "unknown variable `it`"]);
}

/// Capabilities and concurrency: `spawn` outside a `scope`, what a task can't
/// leave, `select` and `race` arms, senders never closed, a capability made
/// by the program, a test asking for a non-capability, a `pub extern`.
const CAPABILITIES: &str = r#"capability NightLamp {
  fn on() -> Int { 1 }
}
type Pt = { x: Int }
pub extern c fn cabs(x: I64) -> I64
fn work(n: Int) -> Int { n + 1 }
fn opt(n: Int) -> Option[Int] { Some(n) }
fn loose() -> Int {
  h := spawn work(1)
  await h
}
fn sel(tx: Sender[Int], rx: Receiver[Int]) -> Int {
  scope {
    h := spawn work(2)
    select {
      work(3) as y => 1,
      tx.send(1) as z => 2,
      await h as r => r,
    }
  }
}
fn empty() -> Int {
  select { }
}
fn none() -> Int {
  race { }
}
fn tasks() -> Int {
  scope {
    a := spawn { return 1 }
    b := spawn { opt(2)? }
    c := spawn { xs := [1, 2].map(x -> spawn work(x))
      0 }
    await a
  }
}
fn chans() -> Int {
  scope {
    (tx, rx) := channel(1)
    (ty, ry) := channel(1)
    h := spawn { tx.send(1)
      0 }
    g := spawn { ty.send(1)
      ty.close()
      0 }
    ty.close()
    1
  }
}
fn racer() -> Int {
  race {
    work(1) as a => a,
    { return 2 } as b => b,
  }
}
fn lamp() -> Int {
  l := NightLamp
  1
}
test "asks" (n: Int, p: Pt, l: NightLamp) { assert true }
fn main() { print(1) }
"#;

#[test]
fn union_arms_as_in_rust() {
    same_errors("union-arms", UNION_ARMS, &["expected `"]);
}

#[test]
fn whole_value_and_assigning_arms_as_in_rust() {
    same_errors("whole-arms", WHOLE_ARMS, &["expected `"]);
}

/// The compiler's errors on `text` are `strata check`'s, on the lines holding one of `keys`.
fn same_errors(name: &str, text: &str, keys: &[&str]) {
    let dir = std::env::temp_dir().join(format!("strata-check-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let prog = dir.join(format!("{name}.strata"));
    save(&prog, text);
    let got = compiler::run(&[std::ffi::OsStr::new("check"), prog.as_os_str()]);
    let got: String = got.lines().filter(|l| keys.iter().any(|k| l.contains(k))).map(|l| format!("{l}\n")).collect();
    let o = Command::new(STRATA).args(["--file", &prog.to_string_lossy(), "check"]).output().unwrap();
    let all = String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr);
    let want: String = all.lines().filter(|l| keys.iter().any(|k| l.contains(k))).map(|l| format!("{l}\n")).collect();
    assert!(!want.is_empty(), "{all}");
    assert_eq!(got, want);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn capabilities_and_concurrency_as_in_rust() {
    same_errors(
        "capabilities",
        CAPABILITIES,
        &["outside a `scope`", "can't leave a task", "can't return from a task", "needs at least one arm", "arm waits on", "gives nothing to name", "never closed here", "holds a copy of", "is a capability", "test parameter", "never `pub`"],
    );
}
