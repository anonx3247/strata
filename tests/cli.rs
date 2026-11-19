use std::io::Write;
use std::process::{Command, Stdio};

fn strata(args: &[&str], dir: &std::path::Path, stdin: Option<&str>) -> (bool, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_strata"))
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(s) = stdin {
        c.stdin.take().unwrap().write_all(s.as_bytes()).unwrap();
    }
    let o = c.wait_with_output().unwrap();
    (o.status.success(), String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr))
}

fn example() -> String {
    format!("{}/examples/orders.strata", env!("CARGO_MANIFEST_DIR"))
}

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("strata-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn example_checks_tests_and_verifies() {
    let d = tmp("example");
    let ex = example();
    let (ok, out) = strata(&["--file", &ex, "check"], &d, None);
    assert!(ok, "{out}");
    let (ok, out) = strata(&["--file", &ex, "test"], &d, None);
    assert!(ok && out.contains("4 passed"), "{out}");
    let (ok, out) = strata(&["--file", &ex, "verify", "--cases", "50"], &d, None);
    assert!(ok && out.contains("incomplete"), "{out}");
}

#[test]
fn render_round_trips() {
    let d = tmp("render");
    let (_, r1) = strata(&["--file", &example(), "render"], &d, None);
    std::fs::write(d.join("r1.strata"), &r1).unwrap();
    let (_, r2) = strata(&["--file", "r1.strata", "render"], &d, None);
    assert_eq!(r1, r2);
}

#[test]
fn static_errors_are_reported() {
    let d = tmp("errors");
    let src = std::fs::read_to_string(example()).unwrap()
        + r#"
fn status(o: Order) -> String { match o { Pending { .. } => "p", Shipped { .. } => "s" } }
fn sneaky(id: OrderId) -> Bool { id.load(id) is Some }
fn bad_qty() -> Line { Line { sku: "x", qty: 1200, unit_price: 5 } }
"#;
    std::fs::write(d.join("bad.strata"), src).unwrap();
    let (ok, out) = strata(&["--file", "bad.strata", "check"], &d, None);
    assert!(!ok);
    assert!(out.contains("`Cancelled { .. }` is not covered"), "{out}");
    assert!(out.contains("`String` does not implement `Db`"), "{out}");
    assert!(out.contains("refinement violated at compile time"), "{out}");
}

#[test]
fn verify_finds_counterexamples() {
    let d = tmp("cex");
    let src = std::fs::read_to_string(example()).unwrap()
        + "fn promo(amount: Cents, coupon: Int) -> Cents requires coupon >= 0 { amount - coupon }\n";
    std::fs::write(d.join("bug.strata"), src).unwrap();
    let (ok, out) = strata(&["--file", "bug.strata", "verify", "promo"], &d, None);
    assert!(!ok && out.contains("does not satisfy `Cents`"), "{out}");
}

#[test]
fn codebase_edits_are_checked_and_atomic() {
    let d = tmp("db");
    assert!(strata(&["init"], &d, None).0);
    assert!(strata(&["add", &example()], &d, None).0);
    let (ok, out) = strata(&["edit"], &d, Some("fn discount(amount: Cents) -> Cents ensures it <= amount { amount }"));
    assert!(ok && out.contains("implementation only"), "{out}");
    let (ok, out) = strata(&["edit"], &d, Some("fn total(lines: List[Cents]) -> Cents { sum(lines) }"));
    assert!(!ok && out.contains("rejected"), "{out}");
    let (_, render) = strata(&["render"], &d, None);
    assert!(render.contains("fn total(lines: NonEmpty[Line])"), "breaking edit must not be committed");
}

#[test]
fn json_inputs_are_validated_against_refinements() {
    let d = tmp("json");
    let (ok, out) = strata(&["--file", &example(), "run", "total", r#"[[{"sku":"a","qty":0,"unit_price":5}]]"#], &d, None);
    assert!(!ok && out.contains("Line.qty: 0 does not satisfy `Quantity`"), "{out}");
    let (ok, out) = strata(&["--file", &example(), "run", "total", r#"[[{"sku":"a","qty":2,"unit_price":5}]]"#], &d, None);
    assert!(ok && out.trim() == "10", "{out}");
}

#[test]
fn tasks_port_checks_tests_and_verifies() {
    let d = tmp("tasks");
    let ex = format!("{}/examples/tasks", env!("CARGO_MANIFEST_DIR"));
    let (ok, out) = strata(&["--file", &ex, "test"], &d, None);
    assert!(ok && out.contains("0 failed"), "{out}");
    let (ok, out) = strata(&["--file", &ex, "verify", "sim.simulate,core.run_worker,memory.mem_claim", "--cases", "300"], &d, None);
    assert!(ok, "{out}");
    let (ok, out) = strata(&["--file", &ex, "run", "print.demo"], &d, None);
    assert!(ok && out.contains("task-3: Done after 2"), "{out}");
}

#[test]
fn builtin_collections_work() {
    let d = tmp("builtins");
    let ex = format!("{}/examples/builtins.strata", env!("CARGO_MANIFEST_DIR"));
    let (ok, out) = strata(&["--file", &ex, "test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");
    let (ok, out) = strata(&["--file", &ex, "verify"], &d, None);
    assert!(ok, "{out}");
    std::fs::write(d.join("e.strata"), "fn f(x: Int) -> Float { x * 1.5 }\nfn g(b: Bool) -> Int { len(b) }\n").unwrap();
    let (ok, out) = strata(&["--file", "e.strata", "check"], &d, None);
    assert!(!ok && out.contains("no version of `len` accepts (Bool)") && out.contains("expected `Int`, found `Float`"), "{out}");
}

#[test]
fn every_example_round_trips_through_render() {
    let d = tmp("roundtrip");
    for ex in ["bank.strata", "orders.strata", "builtins.strata", "traits.strata", "tasks"] {
        let path = format!("{}/examples/{ex}", env!("CARGO_MANIFEST_DIR"));
        let (_, r1) = strata(&["--file", &path, "render"], &d, None);
        std::fs::write(d.join("r.strata"), &r1).unwrap();
        let (_, r2) = strata(&["--file", "r.strata", "render"], &d, None);
        assert_eq!(r1, r2, "{ex} does not round-trip");
        let (ok, out) = strata(&["--file", "r.strata", "test"], &d, None);
        assert!(ok, "{ex} tests fail after round-trip: {out}");
    }
}

#[test]
fn inline_refinements_in_signatures() {
    let d = tmp("inline");
    std::fs::write(
        d.join("i.strata"),
        "fn pick(level: Int where 5 <= it && it <= 13) -> Positive { level - 4 }\n\
         fn halve_up(n: Natural) -> Int where it * 2 <= n { (n + 1) / 2 }\n",
    )
    .unwrap();
    let (ok, out) = strata(&["--file", "i.strata", "verify"], &d, None);
    assert!(!ok && out.contains("✓ pick") && out.contains("does not satisfy `Int where it * 2 <= n`"), "{out}");
    std::fs::write(d.join("c.strata"), "fn pick(level: Int where 5 <= it && it <= 13) -> Int { level }\nfn caller() -> Int { pick(20) }\n").unwrap();
    let (ok, out) = strata(&["--file", "c.strata", "check"], &d, None);
    assert!(!ok && out.contains("20 does not satisfy `Int where 5 <= it && it <= 13`"), "{out}");
}

#[test]
fn traits_laws_and_distinct_types() {
    let d = tmp("traits");
    let ex = format!("{}/examples/traits.strata", env!("CARGO_MANIFEST_DIR"));
    let (ok, out) = strata(&["--file", &ex, "test"], &d, None);
    assert!(ok && out.contains("0 failed"), "{out}");
    let (ok, out) = strata(&["--file", &ex, "verify"], &d, None);
    assert!(!ok && out.contains("law `associative` does not hold for `Midpoint`") && out.contains("✓ law associative for Money"), "{out}");
    assert!(out.contains("✓ combine_all") && out.contains("✓ receipt"), "bounded generics fuzzed at an implementing type: {out}");
    std::fs::write(
        d.join("e.strata"),
        "type Name = String\n\
         distinct type UserId = String where matches(it, \"u_[0-9]+\")\n\
         distinct type OrderId = String where matches(it, \"o_[0-9]+\")\n\
         trait Priced { fn price(self) -> Int }\n\
         fn mixed(o: OrderId) -> UserId { o }\n\
         fn lit() -> UserId { UserId(\"alice\") }\n\
         fn nobound[T](xs: List[T]) -> Int { sum(xs.map(price)) }\n\
         fn notimpl() -> Int { true.price() }\n",
    )
    .unwrap();
    let (ok, out) = strata(&["--file", "e.strata", "check"], &d, None);
    assert!(!ok, "{out}");
    for needle in [
        "`type Name = String` adds nothing",
        "expected `UserId`, found `OrderId`",
        "\"alice\" does not satisfy `UserId`",
        "`T` is not known to implement `Priced` (add a bound: `[T: Priced]`)",
        "`Bool` does not implement `Priced`",
    ] {
        assert!(out.contains(needle), "missing {needle:?} in:\n{out}");
    }
}

#[test]
fn modules_contracts_and_files() {
    let d = tmp("modules");
    let ex = format!("{}/examples/tasks", env!("CARGO_MANIFEST_DIR"));
    // a folder of files is a set of modules; render --split writes it back identically
    let (ok, out) = strata(&["--file", &ex, "render", "--split", "split"], &d, None);
    assert!(ok, "{out}");
    let (_, a) = strata(&["--file", &ex, "render"], &d, None);
    let (_, b) = strata(&["--file", "split", "render"], &d, None);
    assert_eq!(a, b);
    // the contract shows public definitions only
    let (ok, out) = strata(&["--file", &ex, "interface", "core"], &d, None);
    assert!(ok && out.contains("pub fn run_worker") && !out.contains("same_ids"), "{out}");
    // private definitions are not part of the contract
    std::fs::create_dir_all(d.join("p")).unwrap();
    std::fs::write(d.join("p/a.strata"), "pub fn open() -> Int { secret() }\nfn secret() -> Int { 1 }\n").unwrap();
    std::fs::write(d.join("p/b.strata"), "use a.*\nfn f() -> Int { open() + secret() }\nfn g() -> Int { a.secret() }\n").unwrap();
    let (ok, out) = strata(&["--file", "p", "check"], &d, None);
    assert!(!ok && out.contains("`secret` is private to module `a`") && out.contains("`a.secret` is private to module `a`"), "{out}");
    // the codebase database stores qualified names; a private edit leaves dependents alone
    assert!(strata(&["init"], &d, None).0);
    assert!(strata(&["add", &ex], &d, None).0);
    let (ok, out) = strata(&["edit", "--module", "core"], &d, Some("fn same_ids(claimed: List[TaskId], reported: List[TaskId]) -> Bool { len(claimed) == len(reported) && all(claimed, |c| count(reported, |r| r == c) == 1) && all(reported, |r| contains(claimed, r)) }"));
    assert!(ok && out.contains("core.same_ids") && out.contains("implementation only"), "{out}");
    let (ok, out) = strata(&["test"], &d, None);
    assert!(ok && out.contains("0 failed"), "{out}");
}

#[test]
fn procedural_mutation_is_explicit() {
    let d = tmp("mutation");
    std::fs::write(
        d.join("m.strata"),
        "type Line = { qty: Int where 1 <= it && it <= 99 }\n\
         fn restock(mut line: Line, extra: Int) -> () { line.qty = line.qty + extra }\n\
         fn sum(xs: List[Int]) -> Int {\n  let mut s = 0\n  for x in xs { s = s + x }\n  s\n}\n\
         test \"ok\" {\n  let mut l = Line { qty: 2 }\n  restock(l, 3)\n  assert l.qty == 5\n  assert sum([1, 2, 3]) == 6\n}\n\
         test \"refined\" {\n  let mut l = Line { qty: 98 }\n  restock(l, 5)\n}\n",
    )
    .unwrap();
    let (ok, out) = strata(&["--file", "m.strata", "test"], &d, None);
    assert!(!ok && out.contains("✓ ok") && out.contains("103 does not satisfy"), "{out}");
    std::fs::write(d.join("e.strata"), "fn f() -> Int {\n  let x = 1\n  x = 2\n  x\n}\nfn g() -> Int {\n  let mut y = 0\n  let add = |z| { y = y + z; y }\n  add(1)\n}\n").unwrap();
    let (ok, out) = strata(&["--file", "e.strata", "check"], &d, None);
    assert!(!ok && out.contains("`x`: it is not mutable") && out.contains("a closure cannot change `y`"), "{out}");
}

#[test]
fn loops_chains_and_80_columns() {
    let d = tmp("fmt");
    std::fs::write(
        d.join("l.strata"),
        "fn steps(start: Positive) -> Natural {\n  let mut n = start\n  let mut k = 0\n  loop {\n    if n == 1 { break }\n    n = if n % 2 == 0 { n / 2 } else { 3 * n + 1 }\n    k = k + 1\n  }\n  k\n}\n\
         fn top(xs: List[Int]) -> Int {\n  xs\n    .filter(|x| x > 0)\n    .sort()\n    .reverse()\n    .take(3)\n    .sum()\n}\n\
         test \"t\" {\n  assert steps(6) == 8\n  assert top([5, -1, 9, 2, 7]) == 21\n}\n",
    )
    .unwrap();
    let (ok, out) = strata(&["--file", "l.strata", "test"], &d, None);
    assert!(ok, "{out}");
    let ex = format!("{}/examples/tasks", env!("CARGO_MANIFEST_DIR"));
    let (_, r) = strata(&["--file", &ex, "render"], &d, None);
    let long = r.lines().filter(|l| l.chars().count() > 82).count();
    assert!(long <= 1, "rendered lines over 80 columns: {long}");
}
