//! Programs shared by tests/build.rs (compiled) and tests/eval.rs (run by
//! `compiler/eval`): each one's output is recorded in tests/data/build/expected,
//! under its test's folder and its name (`faults-division`).
#![allow(dead_code)]

use std::path::Path;
use std::process::Command;

/// The `faults` programs.
pub const FAULTS: &[(&str, &str, i32)] = &[
    ("early_return", "fn sign(n: Int) -> Int { if n < 0 { return -1 } else { 0 } }\nfn main() -> () { print(sign(-5)) print(sign(5)) }", 0),
    ("requires", "fn half(n: Int) -> Int requires n % 2 == 0 { n / 2 }\nfn main() -> () { print(half(4)) print(half(3)) }", 101),
    ("ensures", "fn f(x: Int) -> Int ensures it > x { x }\nfn main() -> () { print(f(2)) }", 101),
    ("assert", "fn main() { s := \"a\\\"b\" assert s + \"!\" == \"x\" }", 101),
    ("assignment", "fn main() { mut x: Int where it < 5 = 0 while true { x = x + 1 } }", 101),
    ("division", "fn main() { print(-7 / 2) print(-7 % 2) print(7 / -2) print(1 / 0) }", 101),
    ("floats", "fn main() { print(0.1 + 0.2) print(1000000000000000000000.0) print(0.00000015) print(100.0) print(-5.5 % 2.0) print(1.0 / 0.0) }", 101),
    ("exit_code", "fn main() -> Int { 3 }", 3),
    ("invariant", "fn f(n: Int) -> Int { mut i := 0 while i < n invariant i < 3 { i = i + 1 if i == 1 { continue } } i }\nfn main() { print(f(2)) print(f(5)) }", 101),
    (
        "loops",
        "fn g(n: Int) -> Int { mut i := 0 while i < 10 { i = i + 1 if i == n { break } if i % 2 == 0 { continue } } i }\n\
         fn pick(b: Bool) -> String { match b { true => \"yes\", false => return \"no\" } }\n\
         fn main() { print(g(4)) print(g(40)) print(pick(true) + pick(false)) print(f\"{1.5} {true} {\"q\"} {len(\"héllo\".code_points())}\") }",
        0,
    ),
];

/// The `data` programs.
pub const DATA: &[(&str, &str, i32)] = &[
    ("field_refinement", "type Quantity = Int where 1 <= it && it <= 99\ntype Line = { sku: String, qty: Quantity }\nfn main() -> () { mut l := Line { sku: \"a\", qty: 5 } l.qty = 0 }", 101),
    (
        "tree",
        "type Tree[T] =\n  | Leaf\n  | Node(Tree[T], T, Tree[T])\n\
         fn insert(t: Tree[Int], x: Int) -> Tree[Int] { match t { Leaf => Node(Leaf, x, Leaf), Node(l, v, r) => if x < v { Node(insert(l, x), v, r) } else { Node(l, v, insert(r, x)) } } }\n\
         fn size[T](t: Tree[T]) -> Int { match t { Leaf => 0, Node(l, _, r) => size(l) + 1 + size(r) } }\n\
         fn main() { mut t: Tree[Int] = Leaf for i in range(0, 10) { t = insert(t, (i * 7) % 10) } print(size(t)) print(insert(insert(Leaf, 2), 1)) }",
        0,
    ),
    ("own_methods", "type Item = { name: String, price: Int }\nimpl Item {\n  fn new(name: String) -> Item { Item { name: name, price: 0 } }\n  fn label(self) -> String { f\"{self.name}: {self.price}\" }\n}\nfn main() -> () { print(Item.new(\"cup\").label()) }", 0),
    ("refined_argument", "fn f(o: Option[Positive]) -> Int { o ?? 0 }\nfn main() -> () { print(f(Some(3))) let x: Option[Int] = Some(-2) print(f(x)) }", 101),
    ("main_err", "fn main() -> Result[(), String] { print(\"working\") Err(\"it broke\") }", 1),
    (
        "errors",
        "type E =\n  | NotFound\n  | Bad { why: String }\n\
         fn g(n: Int) -> Result[Int, E] { if n == 0 { Err(NotFound) } else if n < 0 { Err(Bad { why: \"neg\" }) } else { Ok(n) } }\n\
         fn h(n: Int) -> Result[Int, E] { v := g(n)! Ok(v * 2) }\n\
         fn main() { print(h(3)) print(h(0)) print(h(-1)) print(h(0) == Err(NotFound)) (x, y) := (5, \"s\") print(f\"{x}{y}\") }",
        0,
    ),
];

/// The `traits` programs.
pub const TRAITS: &[(&str, &str, i32)] = &[
    ("trait_ensures", "trait Describe {\n  fn describe(self) -> String\n    ensures it != \"\"\n}\ntype Thing = { name: String }\nimpl Describe for Thing {\n  fn describe(self) -> String { self.name }\n}\nfn main() -> () { print(Thing { name: \"a\" }.describe()) print(Thing { name: \"\" }.describe()) }", 101),
    ("variant_impls", "trait Area {\n  fn area(self) -> Int\n    requires true\n}\ntype Shape =\n  | Square { side: Int }\n  | Circle { r: Int }\nimpl Area for Shape {\n  fn area(self) -> Int { 0 }\n}\nimpl Area for Square {\n  fn area(self) -> Int { self.side * self.side }\n}\nfn main() -> () { let shapes: List[Shape] = [Square { side: 3 }, Circle { r: 2 }] print(shapes.map(s -> s.area())) }", 0),
    ("generic_traits", "trait Convert[T] {\n  fn convert(self) -> T\n}\ntype Celsius = { deg: Int }\nimpl Convert[String] for Celsius {\n  fn convert(self) -> String { f\"{self.deg}C\" }\n}\nimpl Convert[Int] for Int {\n  fn convert(self) -> Int { self * 2 }\n}\nfn twice[A: Convert[String]](xs: List[A]) -> List[String] { xs.map(x -> x.convert()) }\nfn main() -> () { print(Celsius { deg: 20 }.convert()) print(twice([Celsius { deg: 1 }])) print(21.convert()) }", 0),
    ("default_methods", "trait Shown {\n  fn shown(self) -> String\n  fn twice(self) -> String { self.shown() + self.shown() }\n}\nimpl Shown for Bool { fn shown(self) -> String { if self { \"yes\" } else { \"no\" } } }\nimpl Shown for String { fn shown(self) -> String { self } }\nfn all_shown[T: Shown](xs: List[T]) -> String { join(xs.map(x -> x.twice()), \" \") }\nfn main() -> () { print(all_shown([true, false])) print(all_shown([\"a\", \"b\"])) }", 0),
    ("method_type_params", "trait Boxed {\n  fn wrap[U](self, other: U) -> (Self, U)\n}\ntype K = { k: Int }\nimpl Boxed for K { fn wrap[U](self, other: U) -> (Self, U) { (self, other) } }\nfn main() -> () { print(K { k: 1 }.wrap(\"x\")) print(K { k: 2 }.wrap(3.5)) }", 0),
];

/// The `dyn` programs.
pub const DYN: &[(&str, &str, i32)] = &[
    ("trait_values_in_lists", "trait Shape {\n  fn area(self) -> Int\n  fn name(self) -> String {\n    \"shape\"\n  }\n}\n\ntype Square = { side: Int }\n\ntype Rect = { w: Int, h: Int }\n\nimpl Shape for Square {\n  fn area(self) -> Int {\n    self.side * self.side\n  }\n\n  fn name(self) -> String {\n    \"square\"\n  }\n}\n\nimpl Shape for Rect {\n  fn area(self) -> Int {\n    self.w * self.h\n  }\n}\n\nfn total(shapes: List[Shape]) -> Int {\n  sum(shapes.map(s -> s.area()))\n}\n\nfn main() -> () {\n  let shapes: List[Shape] = [Square { side: 2 }, Rect { w: 2, h: 5 }]\n  print(total(shapes))\n  print(shapes.map(s -> s.name()))\n  print(shapes)\n  let one: Shape = Square { side: 3 }\n  print(one.area())\n  print(one)\n}", 0),
    ("trait_values_in_fields", "use std.io.{Console, recorder}\n\ntype Worker = { console: Console, count: Int }\n\nfn work(mut w: Worker) -> () {\n  mut w.console.print(f\"job {w.count}\")\n  w.count = w.count + 1\n}\n\nfn main(console: Console) -> () {\n  mut real := Worker { console, count: 1 }\n  work(mut real)\n  work(mut real)\n  mut fake := Worker { console: recorder([]), count: 7 }\n  work(mut fake)\n  print(fake.count)\n}", 0),
    ("trait_values_are_values", "use std.io.{Console, recorder}\n\ntype Worker = { console: Console, count: Int }\n\nfn work(mut w: Worker) -> () {\n  mut w.console.print(f\"job {w.count}\")\n}\n\nfn main() -> () {\n  mut a := Worker { console: recorder([]), count: 1 }\n  mut b := a\n  work(mut a)\n  work(mut a)\n  work(mut b)\n  print(a.console)\n  print(b.console)\n  print(a)\n}", 0),
    ("mut_proven_fits", "fn grow(mut n: Positive) {\n  n = n + 1\n}\n\nfn main() -> () {\n  mut p: Natural = 5\n  grow(mut p)\n  print(p)\n}", 0),
    ("mut_field_breaks_invariant", "fn grow(mut n: Int) {\n  n = n + 1\n}\n\ntype R = { lo: Int, hi: Int }\n  where it.lo <= it.hi\n\nfn main() -> () {\n  mut r := R { lo: 1, hi: 2 }\n  grow(mut r.lo)\n  grow(mut r.lo)\n  print(r)\n}", 101),
];

/// The `collections` programs.
pub const COLLECTION_FAULTS: &[(&str, &str)] = &[
    ("dict_value", "fn f() -> Dict[String, Positive] { Dict { \"a\": 1, \"b\": 0 } }\nfn main() -> () { print(f()) }"),
    ("set_element", "fn g(s: Set[Positive]) -> Int { len(s) }\nfn main() -> () { print(g(Set { 1 })) print(g(Set { 3, -2 })) }"),
    ("dict_key", "fn main() { let d: Dict[NonEmptyString, Int] = Dict { \"x\": 1, \"\": 2 } print(d) }"),
    ("set_assigned", "fn main() { mut s: Set[Natural] = Set { 1 } s = insert(s, -1) }"),
    ("ord", "fn main() { print(ord(\"a\")) print(ord(\"a\\\"\\n\")) }"),
    ("chr", "fn main() { print(chr(55296)) }"),
    ("string_index", "fn main() { print(\"abc\".graphemes()[2]) print(\"abc\".graphemes()[5]) }"),
    ("list_index", "fn main() { print([1, 2][-1]) }"),
];

pub const STRINGS: &str = r#"fn main() {
  cr := chr(13)
  text := "one two" + cr + "\nthree\n\nfour" + cr
  print(lines(text))
  print([lines(""), lines("\n"), lines("a\n"), lines("a\n\nb")])
  print(split("a,,b,", ","))
  print(split("", ","))
  print(split("héllo wörld", "ö"))
  print(trim(chr(160) + " \t x y " + chr(12288) + "\n"))
  print([contains("héllo", "ll"), contains("a", ""), starts_with("Zoë", "Zo"), ends_with("Zoë", "ë"), ends_with("a", "ab")])
  print(reverse("Zoë!"))
  print(chars("añb€😀"))
  print([get("Zoë".graphemes(), 2), get("Zoë".graphemes(), 3), get("Zoë".graphemes(), -1)])
  print([join("héllo".graphemes()[1..3], ""), join(slice("héllo".graphemes(), -5, 2), ""), join(slice("héllo".graphemes(), 3, 1), ""), join(slice("héllo".graphemes(), 2, 100), "")])
  print([byte_slice("héllo", 0, 3), byte_slice("héllo", 3, 100), byte_slice("héllo", -1, 1), byte_len("héllo")])
  print([index_of("añb€", "€"), index_of("abc", "z"), index_of("abc", "")])
  print(replace("a-b-c--", "-", "+=") + "|" + replace("aaa", "aa", "b"))
  print(repeat("ab", 3) + "|" + repeat("x", 0) + "|")
  print([to_upper("straße ǆ ŉ"), to_lower("ΟΔΟΣ Σ ΑΣ. İ"), to_upper("ß"), to_lower("ǅ")])
  print([is_digit("0123"), is_digit(""), is_digit("1a"), is_digit("٣"), is_alpha("añ日本"), is_alpha("a1"), is_space(" \t\n"), is_space(chr(8239)), is_space("")])
  print([ord("A"), ord("€"), ord("😀")])
  print([chr(65), chr(8364), chr(128512)])
  print([parse_int("42"), parse_int(" -7 "), parse_int("+3"), parse_int("1_000_"), parse_int("x"), parse_int("9223372036854775807"), parse_int("-9223372036854775808")])
  print([parse_float("2.5"), parse_float("-1e-3"), parse_float("5."), parse_float(".5"), parse_float("1e"), parse_float("inf"), parse_float("12")])
  x := 3.14159
  n := -42
  s := "héllo"
  print(f"[{x:.2}] [{x:8.3}] [{n:>6}] [{n:<6}] [{n:^7}] [{n:06}] [{n:+}] [{7:+05}] [{s:.2}] [{s:>8}] [{s:*^9}] [{s:3}] [{0.125:.2}] [{true:>6}] [{-0.0:.1}]")
  w := "abc"
  print([w.graphemes()[0], w.graphemes()[2], join(w.graphemes()[1..], ""), join(w.graphemes()[..2], ""), join(w.graphemes()[..], "")])
  xs := [10, 20, 30, 40]
  print([xs[1], xs[3]])
  print([xs[1..3], xs[2..], xs[..1], xs[3..1], xs[-2..10]])
}
"#;

pub const COLLECTIONS: &str = r#"type Color =
  | Red
  | Green
  | Blue

distinct type UserId = Int

type Point = { x: Int, y: Int }

fn tally(words: List[String]) -> Dict[String, Int] {
  mut d: Dict[String, Int] = Dict {}
  for w in words {
    d = insert(d, w, (get(d, w) ?? 0) + 1)
  }
  d
}

fn main() {
  d := Dict { "b": 2, "a": 1, "c": 3 }
  print(d)
  print([len(d), len(Dict[Int, Int] {})])
  print(Dict[Int, String] {})
  print(dict([(3, "c"), (1, "a"), (3, "z"), (2, "b")]))
  print(insert(d, "a", 10))
  print(insert(d, "aa", 5))
  print(remove(d, "b"))
  print(remove(d, "zz"))
  print([get(d, "c"), get(d, "q")])
  print([contains(d, "a"), contains(d, "x")])
  print(keys(d))
  print(values(d))
  print(items(d))
  print(map_values(d, v -> float(v) * 1.5))
  print(map_values(d, v -> f"<{v}>"))
  for (k, v) in d {
    print(f"{k}={v}")
  }
  print(tally(split("the cat the hat the end cat", " ")))
  s := Set { 3, 1, 3, 2 }
  print(s)
  print([len(s), len(Set[String] {})])
  print([contains(s, 2), contains(s, 7)])
  print([insert(s, 0), insert(s, 2), remove(s, 1), remove(s, 9)])
  print(to_list(s))
  print([union(s, Set { 5, 0, 2 }), intersection(s, Set { 2, 3, 9 }), difference(s, Set { 2 })])
  for x in s {
    print(x)
  }
  print(s == Set { 1, 2, 3 })
  print(Dict { 1: "a" } == dict([(1, "a")]))
  print(d != remove(d, "a"))
  print(set(["pear", "apple", "fig", "apple"]))
  print(set([2.5, -1.0, 0.0, -0.5]))
  print(set([Blue, Red, Blue, Green]))
  print(set([UserId(10), UserId(9), UserId(100)]))
  print(set([Point { x: 2, y: 1 }, Point { x: 1, y: 5 }, Point { x: 2, y: 1 }]))
  print(set([(2, "b"), (1, "z"), (2, "a")]))
  print(set([[2, 1], [1], [1, 0]]))
  print(set([Some(3), None, Some(1)]))
  print(Dict { Red: 1, Blue: 2 })
  print(Dict { (1, 2): "p", (0, 9): "q" })
  print(Set { Set { 2, 1 }, Set { 0 } })
  print(Dict { "x": Set { 1 }, "y": Set { } })
  nested := Dict { "k": Dict { 2: "two", 1: "one" } }
  print(nested)
  print(f"{d} {s}")
  print(to_string(s))
  print([d][0])
}
"#;

pub const FIELDS: &str = r#"type Sq = { side: Int, name: String }
type Ci = { name: String, r: Int }

fn label(s: Sq | Ci) -> String { s.name }

fn main() {
  print(label(Ci { name: "c", r: 2 }))
  print(label(Sq { side: 1, name: "q" }))
  mut s: Sq | Ci = Ci { name: "c", r: 2 }
  s.name = "renamed"
  print(s)
  mut t: Sq | Ci = Sq { side: 3, name: "s" }
  t.name = t.name + "!"
  print(t)
  mut pair := (1, "a")
  pair.1 = "b"
  pair.0 = pair.0 + 1
  print(pair)
  mut nested := ((1, 2), "x")
  nested.0.1 = 9
  print(nested)
}
"#;

pub const TRAIT_EQUALITY: &str = r#"trait Area {
  fn area(self) -> Int
}
type Sq = { side: Int }
type Ci = { r: Int }
impl Area for Sq { fn area(self) -> Int { self.side * self.side } }
impl Area for Ci { fn area(self) -> Int { 3 * self.r * self.r } }
impl Area for Int { fn area(self) -> Int { self } }

fn main() {
  let a: Area = Sq { side: 2 }
  let b: Area = Sq { side: 2 }
  let c: Area = Ci { r: 2 }
  let d: Area = 4
  print(a == b)
  print(d == d)
  let xs: List[Area] = [a, c, d]
  print(contains(xs, b))
  let ys: List[Area] = [Sq { side: 2 }, Ci { r: 2 }, 4]
  print(xs == ys)
  for (i, x) in enumerate(xs) { print(f"{i}: {x.area()}") }
  for 0 in [0, 0] { print("zero") }
}
"#;

// a variable declared a union stays one (it was narrowed to its first
// value's type); `List[Int]` is a type pattern
pub const DECLARED_UNIONS: &str = r#"fn describe(v: List[Int] | String) -> String {
  match v {
    List[Int] => f"list {len(v)}",
    String => "text",
  }
}

fn main() {
  mut u: Int | String = 3
  u = "text"
  print(u)
  let w: Int | String = 3
  print(w)
  print(describe([1, 2]))
  print(describe("x"))
  let e: List[Int] | String = []
  match e {
    List[Int] => print("empty"),
    String => print("text"),
  }
}
"#;

pub const UNIONS: &str = r#"type Sq = { side: Int }

fn describe(v: Int | String) -> String {
  match v {
    Int => f"number {v}",
    String => f"text {len(v.code_points())}",
  }
}

fn kind(v: Int | String | Sq | List[Int]) -> String {
  match v {
    0 => "zero",
    Positive => f"positive {v}",
    Int => "negative",
    "" => "empty text",
    String => f"text {v}",
    Sq => f"square {v.side}",
    [] => "empty list",
    [x, ..rest] => f"list from {x}, {len(rest)} more",
  }
}

fn pick(b: Bool) -> Int | String {
  if b { 1 } else { "one" }
}

fn main() {
  print(describe(4))
  print(describe("four"))
  for x in [0, 5, -2] { print(kind(x)) }
  print(kind(""))
  print(kind("hey"))
  print(kind(Sq { side: 3 }))
  print(kind([]))
  print(kind([1, 2]))
  let xs: List[Int | String] = [1, "a", 2]
  print(xs)
  print(pick(true) == pick(true))
  print(pick(true) == pick(false))
  print(to_string(pick(false)))
  print(xs.map(x -> describe(x)))
  let u: Int | String = pick(false)
  print(u)
  print(u is String)
  print(u is Int)
}
"#;

pub const UNION_METHODS: &str = r#"trait Area {
  fn area(self) -> Int
}
type Sq = { side: Int }
type Ci = { r: Int }
impl Area for Sq { fn area(self) -> Int { self.side * self.side } }
impl Area for Ci { fn area(self) -> Int { 3 * self.r * self.r } }
impl Area for Int { fn area(self) -> Int { self } }

fn total(xs: List[Sq | Ci | Int]) -> Int { xs.map(x -> x.area()).fold(0, (a, b) -> a + b) }

fn f(x: Positive | String) -> String { f"{x}" }

fn main() {
  print(total([Sq { side: 2 }, Ci { r: 1 }, 5]))
  let s: Sq | Ci = Ci { r: 2 }
  print(s.area())
  print(f(3))
  print(f("a"))
  let n: Int = 0 - 1
  print(f(n))
}
"#;

/// The `capabilities` programs (`files_and_environment` makes files in its folder).
pub const CAPABILITIES: &[(&str, &str)] = &[
    ("console", "use std.io.Console\nfn greet(mut console: Console, name: String) -> () { mut console.print(f\"hello, {name}\") }\nfn main(mut console: Console) -> () { greet(mut console, \"world\") mut console.eprint(\"to stderr\") mut console.write(\"no newline\") mut console.print(\"\") }"),
    ("mut_params", "fn restock(mut n: Int, extra: Int) { n = n + extra }\ntype Line = { sku: String, qty: Int }\nfn bump(mut l: Line) -> () { l.qty = l.qty + 1 }\nfn main() -> () { mut k := 1 restock(mut k, 5) print(k) mut line := Line { sku: \"a\", qty: 1 } bump(mut line) bump(mut line) print(line) restock(mut line.qty, 10) print(line) }"),
    ("files_and_environment", "use std.io.Console\nuse std.fs.{Disk, FsError}\nuse std.process.Process\nfn main(mut console: Console, mut files: Disk, env: Process) -> Result[(), String] {\n  _ := mut files.remove(\"fsplay/b.txt\")\n  _ := mut files.remove(\"fsplay/a.txt\")\n  _ := mut files.remove(\"fsplay/sub/deep\")\n  _ := mut files.remove(\"fsplay/sub\")\n  _ := mut files.remove(\"fsplay\")\n  print(files.exists(\"fsplay\"))\n  print(mut files.make_dir(\"fsplay/sub/deep\"))\n  print(mut files.write(\"fsplay/a.txt\", \"one\\n\"))\n  print(mut files.append(\"fsplay/a.txt\", \"two\\n\"))\n  print(files.read(\"fsplay/a.txt\"))\n  print(files.read(\"fsplay/missing.txt\"))\n  print(files.read(\"fsplay/sub\"))\n  print(mut files.write(\"fsplay/b.txt\", \"x\"))\n  print(files.list(\"fsplay\"))\n  print(files.list(\"fsplay/a.txt\"))\n  print(mut files.remove(\"fsplay/nope\"))\n  print(env.args())\n  print(env.var(\"HOME\") is Some)\n  print(env.var(\"STRATA_SURELY_UNSET_VAR\"))\n  mut console.print(\"done\")\n  Ok(())\n}"),
    ("one_function_two_consoles", "use std.io.{Console, recorder}\nfn report(mut console: Console, n: Int) -> () {\n  for i in range(0, n) { mut console.print(f\"line {i}\") }\n}\nfn main(mut console: Console) -> () {\n  mut r := recorder([])\n  report(mut r, 3)\n  report(mut console, 2)\n  print(r.out)\n  print(r)\n}"),
];

/// `capabilities-args`: the arguments every program is run with.
pub const ARGS: &str = "use std.process.Process\nfn main(env: Process) -> () { print(env.args()) }";

/// A program's text (rendered), its tests turned into blocks of a `main`.
pub fn tests_as_main(file: &str, skip: &str) -> String {
    let src = Command::new(env!("CARGO_BIN_EXE_strata")).args(["render", "--file"]).arg(Path::new(env!("CARGO_MANIFEST_DIR")).join(file)).output().unwrap();
    let src = String::from_utf8(src.stdout).unwrap();
    let mut decls = String::new();
    let mut main = String::from("fn main() {\n");
    let mut in_test = false;
    for line in src.lines() {
        if line.starts_with("test ") {
            in_test = true;
            main += "  {\n";
        } else if in_test && line == "}" {
            in_test = false;
            main += "  }\n";
        } else if in_test {
            if skip.is_empty() || !line.contains(skip) {
                main += line;
                main += "\n";
            }
        } else {
            decls += line;
            decls += "\n";
        }
    }
    main += "  print(\"all asserts hold\")\n}\n";
    decls + &main
}


/// `lists-math`: examples/math.strata, with a `main` that prints what its test checks.
pub fn math() -> String {
    let math = Command::new(env!("CARGO_BIN_EXE_strata")).args(["render", "--file"]).arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/math.strata")).output().unwrap();
    let math = String::from_utf8(math.stdout).unwrap();
    let math = math[..math.find("test \"values\"").unwrap()].to_string()
        + "fn main() {\n  print([factorial(5), gcd(12, 18), power(2, 10), digit_sum(1234), ackermann(2, 3), triangle(10), isqrt(17), collatz_steps(6)])\n  print(positive_squares([-2, 3, 0, 4]))\n  print(primes(20))\n  print(first_or_one([5, 1]))\n  print(average([]))\n}\n";
    math
}

pub const TRAIT_METHODS: &str = r#"trait Named {
  fn name(self) -> String
}

trait Shape: Named {
  fn area(self) -> Int
  fn bigger(self, other: Self) -> Self
  fn scaled(self, k: Int) -> Self
  fn tag[U](self, extra: U) -> (String, U)
  fn grow(mut self, by: Int)
  fn count(self, mut seen: Int) -> Int
}

type Sq = { side: Int }
type Ci = { r: Int }

impl Named for Sq { fn name(self) -> String { "square" } }
impl Named for Ci { fn name(self) -> String { "circle" } }

impl Shape for Sq {
  fn area(self) -> Int { self.side * self.side }
  fn bigger(self, other: Self) -> Self { if other.side > self.side { other } else { self } }
  fn scaled(self, k: Int) -> Self { Sq { side: self.side * k } }
  fn tag[U](self, extra: U) -> (String, U) { (f"sq{self.side}", extra) }
  fn grow(mut self, by: Int) { self.side += by }
  fn count(self, mut seen: Int) -> Int { seen += 1 seen }
}

impl Shape for Ci {
  fn area(self) -> Int { 3 * self.r * self.r }
  fn bigger(self, other: Self) -> Self { if other.r > self.r { other } else { self } }
  fn scaled(self, k: Int) -> Self { Ci { r: self.r * k } }
  fn tag[U](self, extra: U) -> (String, U) { (f"ci{self.r}", extra) }
  fn grow(mut self, by: Int) { self.r += by }
  fn count(self, mut seen: Int) -> Int { seen += 10 seen }
}

trait Convert[T] {
  fn convert(self) -> T
}
trait Pretty: Convert[String] {
  fn pretty(self) -> String
}
impl Convert[String] for Sq { fn convert(self) -> String { f"<{self.side}>" } }
impl Pretty for Sq { fn pretty(self) -> String { "pretty square" } }

fn make(big: Bool) -> Shape {
  if big { Sq { side: 10 } } else { Ci { r: 1 } }
}

fn main() {
  let shapes: List[Shape] = [Sq { side: 2 }, Ci { r: 1 }]
  for s in shapes {
    print(s.name())
    print(s.area())
    print(s.scaled(3).area())
    print(s.bigger(s.scaled(2)).area())
    print(s.tag(7))
    print(s.tag("x"))
    mut n := 0
    print(s.count(mut n))
    print(n)
  }
  mut g: Shape = Ci { r: 2 }
  mut g.grow(3)
  print(g.area())
  let p: Pretty = Sq { side: 4 }
  print(p.pretty())
  print(p.convert())
  print(make(true).area())
  print(make(false).name())
}
"#;

pub const MEMORY: &str = r#"type Shape =
  | Square { side: Int }
  | Circle { r: Int }

trait Area {
  fn area(self) -> Int
  fn grow(mut self)
}

impl Area for Shape {
  fn area(self) -> Int {
    match self {
      Square { side } => side * side,
      Circle { r } => 3 * r * r,
    }
  }
  fn grow(mut self) {
    self = match self {
      Square { side } => Square { side: side + 1 },
      Circle { r } => Circle { r: r + 1 },
    }
  }
}

type Named = { name: String, tags: List[String] }

type Box = { shape: Area, label: String }

fn first_long(xs: List[String]) -> Option[String] {
  for x in xs {
    if len(x.code_points()) > 3 {
      return Some(x)
    }
  }
  None
}

fn parse_all(xs: List[String]) -> Result[List[Int], String] {
  mut out := []
  for x in xs {
    n := parse_int(x) or { return Err(f"bad {x}") }
    out = push(out, n)
  }
  Ok(out)
}

fn total(xs: List[String]) -> Result[Int, String] {
  ns := parse_all(xs)!
  Ok(ns.fold(0, (a, b) -> a + b))
}

fn rename(mut n: Named, to: String) {
  n.name = to
  n.tags = push(n.tags, "renamed")
}

fn describe(xs: List[Int]) -> String {
  match xs {
    [] => "empty",
    [x] => f"one {x}",
    [1, b, ..rest] => f"one then {b} then {rest}",
    [a, ..rest] => f"starts {a}, {len(rest)} more",
  }
}

fn pick(s: Shape) -> String {
  match s {
    Square { side: 1 } | Circle { r: 1 } => "unit",
    Square { side } | Circle { r: side } => f"size {side}",
  }
}

fn main() {
  // closures capturing strings and lists
  prefix := "item-"
  names := ["a", "bb", "cccc", "ddddd"]
  labelled := names.map(n -> prefix + n)
  print(labelled)
  print(labelled.filter(n -> len(n.code_points()) > 6))
  adder := (x: Int) -> x + len(prefix.code_points())
  print(adder(1))
  // early return inside a loop, Option
  print(first_long(names))
  print(first_long(["a"]))
  // ? / ! and `or { return }`
  print(total(["1", "2", "3"]))
  print(total(["1", "x"]))
  // mut parameters, fields
  mut n := Named { name: "old", tags: ["t"] }
  rename(mut n, "new")
  print(n)
  mut holder := Named { name: "h", tags: [] }
  holder.tags = n.tags
  holder.name = "inner"
  print(holder)
  print(n)
  // trait values in a list and a field, with a mut self call
  let shapes: List[Area] = [Square { side: 2 }, Circle { r: 1 }]
  print(shapes.map(s -> s.area()))
  mut b := Box { shape: Square { side: 3 }, label: "b" }
  mut b.shape.grow()
  print(b.shape.area())
  mut sh: Area = Circle { r: 2 }
  mut sh.grow()
  print(sh.area())
  // list patterns, or-patterns
  print(describe([]))
  print(describe([7]))
  print(describe([1, 2, 3, 4]))
  print(describe([5, 1, 0]))
  print(pick(Square { side: 1 }))
  print(pick(Circle { r: 4 }))
  // loops with break / continue and locals
  mut acc := []
  mut i := 0
  while true {
    i += 1
    s := f"n{i}"
    if i % 2 == 0 {
      continue
    }
    if i > 7 {
      break
    }
    acc = push(acc, s)
  }
  print(acc)
  for w in names {
    t := w + "!"
    if len(w.code_points()) == 2 {
      continue
    }
    if len(w.code_points()) == 5 {
      break
    }
    print(t)
  }
  // dicts and sets
  mut d := Dict { "a": 1 }
  d = insert(d, "b", 2)
  d = insert(d, "a", 3)
  print(d)
  print(get(d, "a"))
  print(keys(d))
  s := Set { "x", "y" }
  print(insert(s, "z"))
  print(contains(s, "x"))
  // strings
  text := "  Hello, World  "
  print(trim(text).to_upper())
  print(split("a,b,c", ","))
  print(replace("aXbXc", "X", "--"))
  print(f"{3.14159:.2} [{42:>6}] [{"ab":<4}]")
  print(chars("héllo").reverse())
  print(parse_float(" 2.5 "))
  print(lines("one\ntwo"))
  // reassigning in a loop
  mut words := []
  for k in 0..5 {
    words = words + [f"w{k}"]
  }
  print(words)
  print(words[1..3])
  print(words.sort_by(w -> w).reverse())
}
"#;

pub const NESTED_ASSIGN: &str = r#"type P = { name: String, n: Int }

fn show2(a: String, b: Int) -> String { f"{a}/{b}" }

fn pop(mut xs: List[String]) -> String {
  x := xs[len(xs) - 1]
  xs = xs[..len(xs) - 1]
  x
}

fn main() {
  mut s := f"first {1}"
  print(show2(s, { s = f"second {2}"; 1 }))
  print(s)
  mut xs := [f"a{1}", f"b{2}"]
  print([xs, { xs = [f"c{3}"]; [] }])
  print(xs[0] + { xs = []; "!" })
  mut p := P { name: f"p{1}", n: 1 }
  q := P { ..p, n: { p = P { name: f"z{9}", n: 9 }; 2 } }
  print(q)
  mut ws := [f"w{1}", f"w{2}"]
  print((ws, mut ws.pop()))
  print(ws)
}
"#;

pub const BIG_INTEGERS: &str = r#"fn factorial(n: Int) -> Int { if n <= 1 { 1 } else { n * factorial(n - 1) } }

fn fib(n: Int) -> Int {
  mut a := 0
  mut b := 1
  for _ in 0..n {
    t := a + b
    a = b
    b = t
  }
  a
}

fn main() {
  print(factorial(25))
  print(factorial(30) / factorial(28))
  print(fib(100))
  print(fib(100) % 1000007)
  big := 123456789012345678901234567890
  print(big)
  print(-big)
  print(big * big)
  print(big / -7)
  print(big % -7)
  print(-big / 7)
  print(-big % 7)
  print(big == 123456789012345678901234567890)
  print(big > fib(100))
  print(big < -big)
  print(abs(-big))
  print(min(big, 3))
  print(max(-big, -3))
  print(float(big))
  print(floor(1000000000000000000000000000000.0))
  print(parse_int("99999999999999999999999"))
  print(parse_int("-4611686018427387905"))
  n := 4611686018427387903
  print(n + 1)
  print(n + 1 - 1 == n)
  print([big, 1, -big].sort())
  print(Set { big, big * 1, 2 })
  print(Dict { big: "big" })
  print(f"{big:>40}")
  print(9223372036854775807 + 1)
  print(-9223372036854775808 - 1)
  print(abs(-4611686018427387904))
  match n + 1 {
    4611686018427387904 => print("matched big"),
    _ => print("no"),
  }
  print(2 * 3 + 4 - 5)
  print(len([1, 2, 3]) * 1000000000000000000000)
  // big integers are freed: this loop's thousands of them leave none live
  mut acc: Int = big
  mut i: Int = 0
  while i < 2000 {
    acc = (acc * big + i) % (big * big)
    i = i + 1
  }
  print(acc > 0)
  ys := [big * 2, big * 3]
  zs := ys.push(big * 4)
  print(len(zs))
}
"#;

/// `floats-checked`: the float operations that give `None` where `+`, `*`,
/// `floor`, ... fault (a result that isn't a finite number), and the fault
/// of the operation itself, which `checked_mul` avoids.
pub const CHECKED_FLOATS: &str = r#"fn main() {
  big := 1000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000.0
  print([checked_add(1.5, 2.0), checked_mul(big, 10.0), checked_mul(big, big), checked_div(1.0, 0.0), checked_div(0.0, 0.0), checked_sub(0.5, 2.0), checked_rem(-5.5, 2.0), checked_rem(1.0, 0.0)])
  print([checked_sqrt(2.0), checked_sqrt(-1.0), checked_sqrt(0.0)])
  print([checked_floor(-2.5), checked_ceil(-2.5), checked_round(2.5), checked_round(-0.5)])
  print([f"{0.125:.2}", f"{2.675:.2}", f"{-0.0:.1}", f"{999.9995:.3}"])
  print(sqrt(2.25))
  print(big * big)
}
"#;
