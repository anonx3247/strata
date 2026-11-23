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

const TRAIT_VALUES: &str = r#"trait Shape {
  fn area(self) -> Int
}
trait Named: Shape {
  fn name(self) -> String
}
type Sq = { s: Int }
type Pt = { x: Int }
impl Shape for Sq {
  fn area(self) -> Int { self.s }
}
impl Named for Sq {
  fn name(self) -> String { "sq" }
}
fn a(xs: List[Shape]) -> String { (xs[0] ?? Sq { s: 1 }).area() }
fn b() -> List[Shape] { [Sq { s: 1 }, Pt { x: 2 }] }
fn c(s: Shape) -> String { s.area() }
fn d(s: Shape) -> Int { s }
fn e[T: Named](x: T) -> String { x.area() }
fn f(q: Sq) -> Shape { q }
fn main() { print(1) }
"#;

const DECLARATIONS: &str = r#"type Box[T] = { item: T }
type Sh =
  | Sq(Int)
  | Rc { w: Int, h: Int }
trait Area {
  fn area(self) -> Int
  fn name(self) -> String
}
type Pt = { x: Int }
impl Area for Pt {
  fn area(self) -> Int { 1 }
  fn extra(self) -> Int { 2 }
}
impl Pt {
  fn dup(self) -> Int { 1 }
  fn dup(self) -> Int { 2 }
}
trait Bad: Pt {
  fn f(self) -> Int
}
fn a(x: Nope) -> Int { 1 }
fn b(x: Box[Int, Int]) -> Int { 1 }
fn c() -> Int { Ghost(1) }
fn d(s: Sh) -> Int { match s { Sq(a, b) => a, Rc { w } => w } }
fn e(s: Sh) -> Int { match s { Sq(n) | Rc { w: m, .. } => 1 } }
fn f() -> Self { 1 }
fn g() -> Int {
  break
  1
}
fn h[T: Ghosty](x: T) -> Int { 1 }
fn k(s: Sh) -> Int { match s { Zap => 1, _ => 2 } }
fn main() { print(1) }
"#;

const IMPORTS: &str = r#"from std.utf8 use encode, decode
use std.unicode
fn a() -> String { encode("x") }
fn b() -> List[Int] { encode(1) }
fn c(bs: List[Int]) -> String { decode(bs) }
fn d() -> String { unicode.max_code() }
fn e() -> Int { unicode.next_code("a") }
fn main() { print(1) }
"#;

const UNREACHABLE: &str = r#"type Sh =
  | Sq(Int)
  | Rc(Int, Int)
  | Dot
trait T {
  fn go(self) -> Int
}
fn go() -> Int { 1 }
fn a(s: Sh) -> Int { match s { Sq(_) => 1, Rc(_, _) => 2, Dot => 3, _ => 4 } }
fn b(s: Sh) -> Int { match s { _ => 1, Dot => 2 } }
fn c(s: Sh) -> Int { match s { Dot => 1, Dot => 2, _ => 3 } }
fn d(b: Bool) -> Int { match b { true => 1, false => 2, _ => 3 } }
fn e(o: Option[Int]) -> Int { match o { Some(n) => n, None => 0, x => 1 } }
fn f(n: Int) -> Int { match n { 1 => 1, k => 2, 3 => 3 } }
fn main() { print(1) }
"#;

const MISUSE: &str = r#"fn r() -> Result[Int, String] { Ok(1) }
fn o() -> Option[Int] { None }
fn a() -> Int { 3 except { 0 } }
fn b() -> Int { o() except { 0 } }
fn c() -> Result[Int, String] { n := 3!
  Ok(n) }
fn d() -> Option[Int] { n := r()?
  Some(n) }
fn e() -> Int {
  for x in 5 { print(x) }
  0
}
fn f(n: Int) -> Int { n.size }
fn g(n: Int) -> Int { n(2) }
fn h(n: Int) -> String { f"{n:.2}" }
fn k(s: String) -> String { f"{s:+}" }
fn main() { print(1) }
"#;

const EARLY: &str = r#"fn r() -> Result[Int, String] { Ok(1) }
fn o() -> Option[Int] { None }
fn a() -> Int { r()! }
fn b() -> Int { o()? }
fn c() -> Option[Int] { Some(r()!) }
fn d() -> Result[Int, Int] { Ok(r()!) }
fn main() { print(1) }
"#;

const TASKS: &str = r#"fn work() -> Int { 1 }
fn a() -> Int { await 3 }
fn b() -> String {
  scope {
    h := spawn work()
    await h
  }
}
fn c() -> Int {
  scope {
    h := spawn work()
    h
  }
}
fn main() { print(1) }
"#;

const HANDLES: &str = r#"fn work() -> Int { 1 }
fn a() -> Int {
  mut keep := [spawn work()]
  scope {
    h := spawn work()
    keep = [h]
    await h
  }
}
fn b() -> Int {
  scope {
    h := spawn work()
    f := () -> await h
    f()
  }
}
fn c() -> Int {
  scope {
    h := spawn work()
    g := spawn (await h)
    await g
  }
}
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
    same_errors("clashes", CLASHES);
}

#[test]
fn unknown_names_as_in_rust() {
    same_errors("unknown", UNKNOWN);
}

#[test]
fn arity_as_in_rust() {
    same_errors("arity", ARITY);
}

#[test]
fn types_as_in_rust() {
    same_errors("types", TYPES);
}

#[test]
fn statement_types_as_in_rust() {
    same_errors("statements", STATEMENTS);
}

#[test]
fn record_and_sum_types_as_in_rust() {
    same_errors("records", RECORDS);
}

#[test]
fn generics_as_in_rust() {
    same_errors("generics", GENERICS);
}

#[test]
fn non_exhaustive_matches_as_in_rust() {
    same_errors("matches", MATCHES);
}

#[test]
fn methods_as_in_rust() {
    same_errors("methods", METHODS);
}

#[test]
fn trait_methods_as_in_rust() {
    same_errors("traits", TRAIT_METHODS);
}

#[test]
fn sum_variables_as_in_rust() {
    same_errors("sums", SUMS);
}

#[test]
fn builtin_calls_as_in_rust() {
    same_errors("builtins", BUILTINS);
}

#[test]
fn tuples_and_loops_as_in_rust() {
    same_errors("tuples", TUPLES);
}

#[test]
fn results_and_indexes_as_in_rust() {
    same_errors("results", RESULTS);
}

#[test]
fn narrowed_variables_as_in_rust() {
    same_errors("narrowed", NARROWED);
}

#[test]
fn bounds_and_generic_impls_as_in_rust() {
    same_errors("bounds", TRAITS);
}

#[test]
fn mut_rules_as_in_rust() {
    same_errors("muts", MUTS);
}

#[test]
fn unions_and_refinements_as_in_rust() {
    same_errors("unions", UNIONS);
}

#[test]
fn static_calls_and_unknown_methods_as_in_rust() {
    same_errors("static", STATIC);
}

#[test]
fn contracts_and_refinements_as_in_rust() {
    same_errors("contracts", CONTRACTS);
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
    same_errors("union-arms", UNION_ARMS);
}

#[test]
fn whole_value_and_assigning_arms_as_in_rust() {
    same_errors("whole-arms", WHOLE_ARMS);
}

#[test]
fn trait_values_and_supertraits_as_in_rust() {
    same_errors("trait-values", TRAIT_VALUES);
}

#[test]
fn names_and_declarations_as_in_rust() {
    same_errors("declarations", DECLARATIONS);
}

#[test]
fn imported_functions_as_in_rust() {
    same_errors("imports", IMPORTS);
}

#[test]
fn unreachable_arms_and_method_clashes_as_in_rust() {
    same_errors("unreachable", UNREACHABLE);
}

#[test]
fn misused_values_as_in_rust() {
    same_errors("misuse", MISUSE);
}

#[test]
fn early_errors_as_in_rust() {
    same_errors("early", EARLY);
}

#[test]
fn task_handles_as_in_rust() {
    same_errors("tasks", TASKS);
}

#[test]
fn handles_kept_in_their_scope_as_in_rust() {
    same_errors("handles", HANDLES);
}

/// The compiler's errors on `text` are `strata check`'s, all of them (in
/// any order).
fn same_errors(name: &str, text: &str) {
    let dir = std::env::temp_dir().join(format!("strata-check-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let prog = dir.join(format!("{name}.strata"));
    save(&prog, text);
    let got = compiler::run(&[std::ffi::OsStr::new("check"), prog.as_os_str()]);
    let errors = |out: &str| -> Vec<String> {
        let mut v: Vec<String> = out.lines().filter(|l| l.starts_with("error in ") || l.starts_with("  ")).map(String::from).collect();
        v.sort();
        v
    };
    let o = Command::new(STRATA).args(["--file", &prog.to_string_lossy(), "check"]).output().unwrap();
    let all = String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr);
    assert!(!errors(&all).is_empty(), "{all}");
    assert_eq!(errors(&got), errors(&all));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn capabilities_and_concurrency_as_in_rust() {
    same_errors("capabilities", CAPABILITIES);
}

/// `strata check --compare` runs both checkers on a script: they agree on
/// its errors, so it exits 0 and says so.
#[test]
fn compare_flag_agrees_on_a_script() {
    let cc = std::env::var("STRATA_CC").unwrap_or_else(|_| "clang".into());
    if !Command::new(cc).arg("--version").output().is_ok_and(|o| o.status.success()) {
        eprintln!("skipped: no clang");
        return;
    }
    let dir = std::env::temp_dir().join(format!("strata-check-test-compare-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let prog = dir.join("compare.strata");
    save(&prog, TYPES);
    // the compiler is built once into this cache, shared by later runs
    let cache = std::env::temp_dir().join("strata-compare-test-cache");
    let o = Command::new(STRATA).args(["--file", &prog.to_string_lossy(), "check", "--compare"]).env("STRATA_CACHE", &cache).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(0), "{out}");
    assert!(out.contains("compare: they agree (") && !out.contains("only in"), "{out}");
    assert!(out.contains("error in f:"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
    // a package: its modules checked together
    let pkg = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/tasks");
    let o = Command::new(STRATA).args(["--file", &pkg.to_string_lossy(), "check", "--compare"]).env("STRATA_CACHE", &cache).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(0), "{out}");
}
