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
    assert!(out.contains("`Cancelled` is not covered"), "{out}");
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

/// `name` passed `verify`: proven, or (without a solver) fuzzed.
fn passed(out: &str, name: &str) -> bool {
    out.contains(&format!("✓ {name} ")) || out.contains(&format!("~ {name} "))
}

fn has_solver() -> bool {
    let bin = std::env::var("STRATA_SOLVER").unwrap_or("z3".into());
    Command::new(bin).arg("--version").output().is_ok_and(|o| o.status.success())
}

#[test]
fn ints_grow_past_64_bits() {
    let d = tmp("bigint");
    let src = "fn fact(n: Natural) -> Positive { if n == 0 { 1 } else { n * fact(n - 1) } }
fn wrap(x: U8) -> U8 { x + 1 }
fn neg(x: I8) -> I8 { 0 - x }
fn widen(x: U64) -> U64 { x + 1 }
test \"big integers\" {
  assert fact(25) == 15511210043330985984000000
  assert fact(25) / fact(24) == 25
  assert (0 - 100000000000000000000) % 7 == 5
  assert (0 - 100000000000000000000) / 7 == -14285714285714285715
  assert 9223372036854775807 + 1 == 9223372036854775808
  assert abs(0 - 9223372036854775807 - 1) == 9223372036854775808
  assert parse_int(\"123456789012345678901234567890\") == Some(123456789012345678901234567890)
  assert sort([100000000000000000000, 3, 0 - 100000000000000000000]) == [0 - 100000000000000000000, 3, 100000000000000000000]
  assert f\"{fact(21)}\" == \"51090942171709440000\"
}
";
    std::fs::write(d.join("b.strata"), src).unwrap();
    let (ok, out) = strata(&["--file", "b.strata", "test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");
    // larger than 64 bits in JSON: a number up to 2^64 - 1, or a string of digits
    let (ok, out) = strata(&["--file", "b.strata", "run", "widen", "[18446744073709551614]"], &d, None);
    assert!(ok && out.contains("\"18446744073709551615\""), "{out}");
    let (ok, out) = strata(&["--file", "b.strata", "run", "widen", "[\"18446744073709551615\"]"], &d, None);
    assert!(!ok && out.contains("18446744073709551616") && out.contains("does not satisfy `U64`"), "{out}");
    if has_solver() {
        let (ok, out) = strata(&["--file", "b.strata", "verify", "wrap,neg,widen", "--cases", "20"], &d, None);
        assert!(!ok && out.contains("input:   [255]") && out.contains("input:   [-128]") && out.contains("input:   [\"18446744073709551615\"]"), "sized types catch overflow: {out}");
    }
    // render keeps big literals
    let (_, r) = strata(&["--file", "b.strata", "render"], &d, None);
    assert!(r.contains("15511210043330985984000000"), "{r}");
}

#[test]
fn cross_check_catches_a_wrong_prover() {
    // a "solver" that proves everything: the cross-check must still catch the bug
    let d = tmp("cross-check");
    let fake = d.join("fake-solver");
    std::fs::write(&fake, "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo fake 1.0; exit 0; fi\ncat > /dev/null\necho unsat\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(d.join("b.strata"), "type Quantity = Int where 1 <= it && it <= 99\nfn bump(q: Quantity) -> Quantity { q + 1 }\n").unwrap();
    let run = |extra: &[&str]| {
        let o = Command::new(env!("CARGO_BIN_EXE_strata"))
            .args(["--file", "b.strata", "verify"])
            .args(extra)
            .env("STRATA_SOLVER", &fake)
            .current_dir(&d)
            .output()
            .unwrap();
        (o.status.success(), String::from_utf8_lossy(&o.stdout).to_string())
    };
    let (ok, out) = run(&[]);
    assert!(!ok && out.contains("✗ bump") && out.contains("found by the cross-check") && out.contains("a bug in the prover"), "{out}");
    let (ok, out) = run(&["--cross-check", "0"]);
    assert!(ok && out.contains("✓ bump"), "without the cross-check the wrong proof stands: {out}");
}

#[test]
fn package_verify_policies() {
    if !has_solver() {
        eprintln!("z3 is not installed: skipping");
        return;
    }
    let d = tmp("policy");
    std::fs::write(d.join("strata.toml"), "name = \"pol\"\n\n[verify]\nterminates = [\"*\"]\nno_recursion = [\"core\"]\nproven = [\"core\"]\n").unwrap();
    std::fs::write(
        d.join("core.strata"),
        "pub fn fact(n: Natural) -> Positive { if n == 0 { 1 } else { n * fact(n - 1) } }\npub fn spin(n: Int) -> Int {\n  let mut k = n\n  loop {\n    if k == 1 { break }\n    k = k - 1\n  }\n  k\n}\n",
    )
    .unwrap();
    std::fs::write(d.join("main.strata"), "use core.*\nfn fib(n: Natural) -> Natural { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }\n").unwrap();
    let (ok, out) = strata(&["--file", ".", "verify", "--cases", "20"], &d, None);
    assert!(!ok && out.contains("policy: `core.fact` is recursive (strata.toml [verify])") && out.contains("policy: `core.spin` may not terminate"), "{out}");
    assert!(!out.contains("`fib` is recursive"), "no_recursion covers only `core`: {out}");
    // --smt shows the termination queries too
    let (_, out) = strata(&["--file", ".", "verify", "fib", "--smt"], &d, None);
    assert!(out.contains("termination: measure (n) goes down at recursive call 1") && out.contains("termination: measure (n) goes down at recursive call 2"), "{out}");
    // a rule must name a module of the package
    std::fs::write(d.join("strata.toml"), "name = \"pol\"\n\n[verify]\nproven = [\"nope\"]\n").unwrap();
    let (ok, out) = strata(&["ci"], &d, None);
    assert!(!ok && out.contains("[verify] proven names `nope`, which is not a module of this package"), "{out}");
}

#[test]
fn termination_is_shown_or_flagged() {
    if !has_solver() {
        eprintln!("z3 is not installed: skipping");
        return;
    }
    let d = tmp("termination");
    let src = "fn fact(n: Natural) -> Positive { if n == 0 { 1 } else { n * fact(n - 1) } }
fn gcd(a: Natural, b: Natural) -> Natural { if b == 0 { a } else { gcd(b, a % b) } }
fn length(xs: List[Int]) -> Natural { match xs { [] => 0, [_, ..rest] => 1 + length(rest) } }
fn count_up(n: Natural) -> Natural {
  let mut i = 0
  while i < n {
    i = i + 1
  }
  i
}
fn forever(n: Int) -> Int { forever(n) }
fn spin(n: Int) -> Int {
  let mut k = n
  loop {
    if k == 1 { break }
    k = k - 1
  }
  k
}
fn trusted(n: Int) -> Int
  terminates
{
  spin(n)
}
fn uses_spin(n: Int) -> Int { spin(n) + 1 }
fn uses_trusted(n: Int) -> Int { trusted(n) + 1 }
fn up(n: Int) -> Int { if n > 100 { n } else { up(n + 1) } }
fn search(xs: List[Int], t: Int, lo: Int, hi: Int) -> Bool {
  if lo >= hi {
    false
  } else {
    let mid = (lo + hi) / 2
    match get(xs, mid) {
      Some(x) => if x < t { search(xs, t, mid + 1, hi) } else { x == t || search(xs, t, lo, mid) },
      None => false,
    }
  }
}
fn ack(m: Natural, n: Natural) -> Natural {
  if m == 0 { n + 1 } else { if n == 0 { ack(m - 1, 1) } else { ack(m - 1, ack(m, n - 1)) } }
}
fn total(xs: List[Int]) -> Int { if len(xs) == 0 { 0 } else { total(drop(xs, 1)) } }
fn skip2(n: Int) -> Int { if n == 0 { 0 } else { skip2(n - 2) } }
";
    std::fs::write(d.join("t.strata"), src).unwrap();
    let (_, out) = strata(&["--file", "t.strata", "verify", "--checks", "--cases", "20"], &d, None);
    let after = |name: &str| -> String {
        let mut lines = out.lines().skip_while(|l| l.split_whitespace().nth(1) != Some(name));
        lines.next();
        lines.take_while(|l| l.starts_with("    ")).collect::<Vec<_>>().join("\n")
    };
    let measured = |name: &str, m: &str| after(name).contains(&format!("terminates: {m} goes down on every recursive call"));
    assert!(measured("fact", "`n`") && measured("gcd", "`b`") && measured("up", "`100 - n`") && measured("search", "`hi - lo`"), "{out}");
    assert!(measured("ack", "the pair (`m`, `n`), compared in order,") && measured("total", "`len(xs)`"), "{out}");
    assert!(after("skip2").contains("may not terminate"), "skip2(1) never ends: {out}");
    assert!(after("length").contains("smaller piece of `xs`"), "{out}");
    assert!(after("count_up").contains("`n - i` goes down"), "{out}");
    assert!(after("forever").contains("may not terminate: it calls itself"), "{out}");
    assert!(after("spin").contains("may not terminate: it has a `loop`"), "{out}");
    assert!(after("uses_spin").contains("it calls `spin`, which may not terminate"), "{out}");
    assert!(after("uses_trusted").contains("terminates if `trusted` does (marked `terminates`"), "{out}");
    // policies
    let (ok, out) = strata(&["--file", "t.strata", "verify", "--require", "terminates", "--cases", "20"], &d, None);
    assert!(!ok && out.contains("policy: `spin` may not terminate") && !out.contains("policy: `uses_trusted`"), "{out}");
    let (ok, out) = strata(&["--file", "t.strata", "verify", "fact,count_up", "--require", "no-recursion"], &d, None);
    assert!(!ok && out.contains("policy: `fact` is recursive") && !out.contains("`count_up` is recursive"), "{out}");
    let (ok, out) = strata(&["--file", "t.strata", "verify", "count_up", "--require", "terminates,proven"], &d, None);
    assert!(ok, "{out}");
    // `terminates` round-trips
    let (_, r) = strata(&["--file", "t.strata", "render"], &d, None);
    assert!(r.contains("fn trusted(n: Int) -> Int\n  terminates {"), "{r}");
    std::fs::write(d.join("r.strata"), &r).unwrap();
    let (_, r2) = strata(&["--file", "r.strata", "render"], &d, None);
    assert_eq!(r, r2);
}

#[test]
fn contracts_see_through_helpers_that_terminate() {
    if !has_solver() {
        eprintln!("z3 is not installed: skipping");
        return;
    }
    let d = tmp("transparent");
    let src = "fn small(n: Int) -> Bool { 0 <= n && n < 10 }
type Digit = Int where small(it)
fn zero() -> Digit { 0 }
fn ten() -> Digit { 10 }
fn up(n: Int) -> Bool { if n > 100 { true } else { up(n + 1) } }
type Odd = Int where up(it)
fn odd() -> Odd { 1 }
";
    std::fs::write(d.join("h.strata"), src).unwrap();
    let (_, out) = strata(&["--file", "h.strata", "verify", "zero,ten,odd", "--cases", "20"], &d, None);
    assert!(out.contains("✓ zero") && out.contains("proven"), "a terminating helper is seen through: {out}");
    assert!(out.contains("✗ ten") && out.contains("found by the solver"), "{out}");
    assert!(out.contains("~ odd") && out.contains("the result of `up` (only its contract is known)"), "a helper that may not terminate is not unfolded: {out}");
}

#[test]
fn smt_handles_variants_patterns_and_declared_types() {
    if !has_solver() {
        eprintln!("z3 is not installed: skipping");
        return;
    }
    let d = tmp("smt-patterns");
    let src = "type Shape =
  | Square { side: Positive }
  | Circle { radius: Positive }
fn area(s: Shape) -> Positive { match s { Square { side } => side * side, Circle { radius } => 3 * radius * radius } }
fn grow(c: Circle) -> Circle { Circle { radius: c.radius + 1 } }
fn small(n: Int) -> Natural { match n { 0 | 1 | 2 => n, where it < 0 => 0, _ => 3 } }
fn bad_small(n: Int) -> Natural { match n { 0 | 1 | -2 => n, _ => 3 } }
fn countdown(start: Natural) -> Natural {
  let mut k: Natural = start
  while k > 0 {
    k = k - 1
  }
  k
}
fn bad_countdown(start: Natural) -> Int {
  let mut k: Natural = start
  k = k - 1
  k
}
";
    std::fs::write(d.join("p.strata"), src).unwrap();
    let (ok, out) = strata(&["--file", "p.strata", "verify", "--cases", "20"], &d, None);
    assert!(!ok, "{out}");
    for name in ["area", "grow", "small", "countdown"] {
        assert!(out.contains(&format!("✓ {name} ")), "{name} should be proven: {out}");
    }
    assert!(out.contains("✗ bad_small") && out.contains("input:   [-2]"), "{out}");
    assert!(out.contains("✗ bad_countdown") && out.contains("assigning `k`: -1 does not satisfy `Natural`"), "a declared type holds after assignment: {out}");
    // what a `mut` argument held before the call is forgotten, field or not
    let src = "type Box = { n: Int }
fn bump(mut x: Int) -> () { x = x - 5 }
fn stale(a: Natural) -> Natural {
  let mut x = a
  bump(mut x)
  x
}
fn stale_field(a: Natural) -> Natural {
  let mut b = Box { n: a }
  bump(mut b.n)
  b.n
}
fn fine(a: Natural) -> Natural {
  let mut x = a
  bump(mut x)
  a
}
";
    std::fs::write(d.join("m.strata"), src).unwrap();
    let (ok, out) = strata(&["--file", "m.strata", "verify", "--cases", "20"], &d, None);
    assert!(!ok && out.contains("✗ stale ") && out.contains("✗ stale_field") && out.contains("✓ fine"), "{out}");
}

#[test]
fn smt_proves_contracts_and_refutes_bugs() {
    if !has_solver() {
        eprintln!("z3 is not installed: skipping");
        return;
    }
    let d = tmp("smt");
    let src = r#"type Cents = Int where it >= 0
type Quantity = Int where 1 <= it && it <= 99
type Code = String where matches(it, "ord_[a-z0-9]{6}")
type Line = { qty: Quantity, unit_price: Cents }

fn dec(n: Int where it > 0) -> Natural { n - 1 }
fn dec2(n: Int where it > 0) -> Natural { n - 2 }
fn discount(amount: Cents) -> Cents
  ensures it <= amount
{
  if amount >= 10000 { amount - amount / 10 } else { amount }
}
fn half(n: Int) -> Int { 100 / n }
fn bump(q: Quantity) -> Quantity { q + 1 }
fn code(s: String where matches(it, "[a-z]{6}")) -> Code { "ord_" + s }
fn bad_code(s: String) -> Code { "ord_" + s }
fn caller(x: Int) -> Natural { if x > 0 { dec(x) } else { 0 } }
fn bad_caller(x: Int) -> Natural { dec(x) }
fn count_up(n: Natural) -> Natural {
  let mut i = 0
  while i < n {
    i = i + 1
  }
  i
}
fn first_qty(xs: List[Quantity]) -> Quantity { match xs { [] => 1, [x, ..] => x } }
fn line_total(l: Line) -> Cents { l.qty * l.unit_price }
fn total(lines: List[Line]) -> Cents { sum(lines.map(line_total)) }
fn checked(a: Int, b: Int) -> Int requires b != 0 { a % b }
fn use_checked(a: Int) -> Int { checked(a, a - 3) }
"#;
    std::fs::write(d.join("s.strata"), src).unwrap();
    let (ok, out) = strata(&["--file", "s.strata", "verify"], &d, None);
    assert!(!ok, "{out}");
    let line = |name: &str| out.lines().find(|l| l.split_whitespace().nth(1) == Some(name)).unwrap_or("").to_string();
    for name in ["dec", "discount", "code", "caller", "count_up", "first_qty", "line_total", "total", "checked"] {
        assert!(line(name).starts_with("✓") && line(name).contains("proven"), "{name} should be proven: {out}");
    }
    for name in ["dec2", "half", "bump", "bad_code", "bad_caller", "use_checked"] {
        assert!(line(name).starts_with("✗") && line(name).contains("found by the solver"), "{name} should be refuted: {out}");
    }
    assert!(out.contains("division by zero") && out.contains("100 does not satisfy `Quantity`"), "{out}");
    // what can't be proven is fuzzed, with the reason
    std::fs::write(d.join("f.strata"), "fn mean(xs: List[Int]) -> Int where it >= 0 { if len(xs) == 0 { 0 } else { abs(xs.fold(0, (a, x) -> a + x)) / len(xs) } }\nfn neg(xs: List[Int]) -> Natural { xs.fold(0, (a, x) -> a + x) }\n").unwrap();
    let (ok, out) = strata(&["--file", "f.strata", "verify"], &d, None);
    assert!(!ok && out.contains("~ mean") && out.contains("not proven: result of `mean`") && out.contains("`fold` is not modelled") && out.contains("✗ neg") && out.contains("found by the fuzzer"), "{out}");
    let (ok, out) = strata(&["--file", "s.strata", "verify", "bad_caller", "--smt"], &d, None);
    assert!(ok && out.contains("(check-sat)") && out.contains("(assert (not (> v0 0)))"), "{out}");
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
    assert!(ok && out.contains("2 passed"), "{out}");
    let (ok, out) = strata(&["--file", &ex, "verify"], &d, None);
    assert!(ok, "{out}");
    std::fs::write(d.join("e.strata"), "fn f(x: Int) -> Float { x * 1.5 }\nfn g(b: Bool) -> Int { len(b) }\n").unwrap();
    let (ok, out) = strata(&["--file", "e.strata", "check"], &d, None);
    assert!(!ok && out.contains("no version of `len` accepts (Bool)") && out.contains("expected `Int`, found `Float`"), "{out}");
}

#[test]
fn every_example_round_trips_through_render() {
    let d = tmp("roundtrip");
    for ex in ["bank.strata", "orders.strata", "builtins.strata", "traits.strata"] {
        let path = format!("{}/examples/{ex}", env!("CARGO_MANIFEST_DIR"));
        let (_, r1) = strata(&["--file", &path, "render"], &d, None);
        std::fs::write(d.join("r.strata"), &r1).unwrap();
        let (_, r2) = strata(&["--file", "r.strata", "render"], &d, None);
        assert_eq!(r1, r2, "{ex} does not round-trip");
        let (ok, out) = strata(&["--file", "r.strata", "test"], &d, None);
        assert!(ok, "{ex} tests fail after round-trip: {out}");
    }
    // a package: one file per module
    let path = format!("{}/examples/tasks", env!("CARGO_MANIFEST_DIR"));
    let (ok, out) = strata(&["--file", &path, "render", "--split", "t1"], &d, None);
    assert!(ok, "{out}");
    let (ok, out) = strata(&["--file", "t1", "render", "--split", "t2"], &d, None);
    assert!(ok, "{out}");
    for m in ["core", "memory", "print", "sim"] {
        let f = format!("{m}.strata");
        assert_eq!(std::fs::read_to_string(d.join("t1").join(&f)).unwrap(), std::fs::read_to_string(d.join("t2").join(&f)).unwrap(), "tasks/{f} does not round-trip");
    }
    let (ok, out) = strata(&["--file", "t2", "test"], &d, None);
    assert!(ok, "tasks tests fail after round-trip: {out}");
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
    assert!(!ok && passed(&out, "pick") && out.contains("does not satisfy `Int where it * 2 <= n`"), "{out}");
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
    assert!(!ok && out.contains("law `associative` does not hold for `Midpoint`") && out.contains("~ law associative for Money"), "{out}");
    assert!(passed(&out, "combine_all") && passed(&out, "receipt"), "bounded generics checked at an implementing type: {out}");
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
    // a folder of files is a set of modules, one per file; render --split writes it back identically
    let (ok, out) = strata(&["--file", &ex, "render", "--split", "split"], &d, None);
    assert!(ok, "{out}");
    for m in ["core", "memory", "print", "sim"] {
        let f = format!("{m}.strata");
        assert_eq!(std::fs::read_to_string(format!("{ex}/{f}")).unwrap(), std::fs::read_to_string(d.join("split").join(&f)).unwrap(), "{f}");
    }
    // `render` prints one module; with several it asks which
    let (ok, out) = strata(&["--file", &ex, "render", "core"], &d, None);
    assert!(ok && out == std::fs::read_to_string(format!("{ex}/core.strata")).unwrap(), "{out}");
    let (ok, out) = strata(&["--file", &ex, "render"], &d, None);
    assert!(!ok && out.contains("4 modules, one file each"), "{out}");
    // `module` blocks are gone: a file is a module
    std::fs::write(d.join("blocks.strata"), "module a {\n  fn f() -> Int {\n    1\n  }\n}\n").unwrap();
    let (ok, _) = strata(&["--file", "blocks.strata", "check"], &d, None);
    assert!(!ok, "`module a {{ }}` must not parse");
    // the contract shows public definitions only
    let (ok, out) = strata(&["--file", &ex, "interface", "core"], &d, None);
    assert!(ok && out.contains("pub fn run_worker") && !out.contains("same_ids"), "{out}");
    // private definitions are not part of the contract
    std::fs::create_dir_all(d.join("p")).unwrap();
    std::fs::write(d.join("p/a.strata"), "pub fn open() -> Int { secret() }\nfn secret() -> Int { 1 }\n").unwrap();
    std::fs::write(d.join("p/b.strata"), "use a.*\nfn f() -> Int { open() + secret() }\nfn g() -> Int { a.secret() }\n").unwrap();
    let (ok, out) = strata(&["--file", "p", "check"], &d, None);
    assert!(!ok && out.contains("`secret` is private to module `a`") && out.contains("`a.secret` is private to module `a`"), "{out}");
    // a package keeps qualified names; a private edit leaves dependents alone
    let pkg = d.join("pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    assert!(strata(&["init"], &pkg, None).0);
    assert!(strata(&["add", &ex], &pkg, None).0);
    for m in ["core", "memory", "print", "sim"] {
        assert!(pkg.join(format!("{m}.strata")).is_file(), "add writes one file per module: {m}");
    }
    let (ok, out) = strata(&["edit", "--module", "core"], &pkg, Some("fn same_ids(claimed: List[TaskId], reported: List[TaskId]) -> Bool { len(claimed) == len(reported) && all(claimed, c -> count(reported, r -> r == c) == 1) && all(reported, r -> contains(claimed, r)) }"));
    assert!(ok && out.contains("core.same_ids") && out.contains("implementation only"), "{out}");
    assert!(std::fs::read_to_string(pkg.join("core.strata")).unwrap().contains("all(reported, r -> contains(claimed, r))"), "the edit is in the file");
    let (ok, out) = strata(&["test"], &pkg, None);
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
         test \"ok\" {\n  let mut l = Line { qty: 2 }\n  restock(mut l, 3)\n  assert l.qty == 5\n  assert sum([1, 2, 3]) == 6\n}\n\
         test \"refined\" {\n  let mut l = Line { qty: 98 }\n  restock(mut l, 5)\n}\n",
    )
    .unwrap();
    let (ok, out) = strata(&["--file", "m.strata", "test"], &d, None);
    assert!(!ok && out.contains("✓ ok") && out.contains("103 does not satisfy"), "{out}");
    std::fs::write(d.join("e.strata"), "fn f() -> Int {\n  let x = 1\n  x = 2\n  x\n}\nfn g() -> Int {\n  let mut y = 0\n  let add = z -> { y = y + z; y }\n  add(1)\n}\n").unwrap();
    let (ok, out) = strata(&["--file", "e.strata", "check"], &d, None);
    assert!(!ok && out.contains("`x`: it is not mutable") && out.contains("a closure cannot change `y`"), "{out}");

    // a call that can change an argument says so: `mut` at the call site
    std::fs::write(
        d.join("c.strata"),
        "type Line = { qty: Int }\n\
         trait Counter { fn tick(mut self) -> () }\n\
         impl Counter for Line { fn tick(mut self) -> () { self.qty = self.qty + 1 } }\n\
         fn restock(mut line: Line, extra: Int) -> () { line.qty = line.qty + extra }\n\
         fn qty(line: Line) -> Int { line.qty }\n\
         fn unmarked() -> Int {\n  let mut l = Line { qty: 1 }\n  restock(l, 2)\n  l.qty\n}\n\
         fn needless() -> Int {\n  let mut l = Line { qty: 1 }\n  qty(mut l)\n}\n\
         fn builtin() -> Int {\n  let mut xs = [1]\n  len(mut xs)\n}\n\
         fn receiver() -> Int {\n  let mut l = Line { qty: 1 }\n  l.tick()\n  restock(mut l, 1)\n  l.qty\n}\n",
    )
    .unwrap();
    let (ok, out) = strata(&["--file", "c.strata", "check"], &d, None);
    assert!(!ok, "{out}");
    assert!(out.contains("in unmarked: `restock` can change its `mut line` parameter: write `mut l` to show that `l` may change"), "{out}");
    assert!(out.contains("in needless: `mut` on an argument of `qty`, but its parameter `line` is not `mut`"), "{out}");
    assert!(out.contains("in builtin:"), "{out}");
    assert!(!out.contains("in receiver"), "{out}");
    let (ok, out) = strata(&["--file", "c.strata", "render"], &d, None);
    assert!(ok && out.contains("restock(mut l, 1)"), "{out}");
}

#[test]
fn loops_chains_and_80_columns() {
    let d = tmp("fmt");
    std::fs::write(
        d.join("l.strata"),
        "fn steps(start: Positive) -> Natural {\n  let mut n = start\n  let mut k = 0\n  loop {\n    if n == 1 { break }\n    n = if n % 2 == 0 { n / 2 } else { 3 * n + 1 }\n    k = k + 1\n  }\n  k\n}\n\
         fn top(xs: List[Int]) -> Int {\n  xs\n    .filter(x -> x > 0)\n    .sort()\n    .reverse()\n    .take(3)\n    .sum()\n}\n\
         test \"t\" {\n  assert steps(6) == 8\n  assert top([5, -1, 9, 2, 7]) == 21\n}\n",
    )
    .unwrap();
    let (ok, out) = strata(&["--file", "l.strata", "test"], &d, None);
    assert!(ok, "{out}");
    let ex = format!("{}/examples/tasks", env!("CARGO_MANIFEST_DIR"));
    let (_, r) = strata(&["--file", &ex, "render"], &d, None);
    let long = r.lines().filter(|l| l.chars().count() > 82).count();
    // a couple of deeply nested lines still exceed 80 columns
    assert!(long <= 2, "rendered lines over 80 columns: {long}");
}

#[test]
fn comments_are_kept_in_the_tree() {
    let d = tmp("comments");
    let src = r#"// file header
//
// second paragraph

// section banner
/// A status.
pub type Status =
  // waiting
  | Pending
  | Done // finished

pub type Point = { x: Int, y: Int } // the y axis points down

pub trait Shape {
  // area, never negative
  /// The area.
  fn area(self) -> Int
    ensures it >= 0
  // checked by verify
  /// Areas are never negative.
  law nonneg(s: Self): s.area() >= 0
}

impl Shape for Point {
  // a point has no area
  /// Always zero.
  fn area(self) -> Int {
    0
  }
}

fn label(s: Status) -> String {
  // the common case first
  let n = 1 // one


  let m = n
  match s {
    // still going
    Pending => "pending",
    Done => "done", // all good
  }
}
"#;
    std::fs::write(d.join("c.strata"), src).unwrap();
    let (ok, r1) = strata(&["--file", "c.strata", "render"], &d, None);
    assert!(ok, "{r1}");
    for c in ["// file header\n//\n// second paragraph\n\n// section banner\n/// A status.", "  // waiting\n  | Pending", "  // finished\n  | Done",
        "// the y axis points down", "  // area, never negative\n  /// The area.", "  // checked by verify\n  /// Areas are never negative.\n  law nonneg", "  // a point has no area\n  /// Always zero.",
        "  // the common case first\n  // one\n  let n = 1\n\n  let m = n\n  match", "    // still going\n    Pending", "    // all good\n    Done"] {
        assert!(r1.contains(c), "missing {c:?} in\n{r1}");
    }
    std::fs::write(d.join("r1.strata"), &r1).unwrap();
    let (_, r2) = strata(&["--file", "r1.strata", "render"], &d, None);
    assert_eq!(r1, r2);
    let (ok, out) = strata(&["--file", "c.strata", "check"], &d, None);
    assert!(ok, "{out}");

    // a comment the tree has no place for is an error, not silently dropped
    std::fs::write(d.join("bad.strata"), "fn f(a: Int, // why\n  b: Int) -> Int { a }\n").unwrap();
    let (ok, out) = strata(&["--file", "bad.strata", "check"], &d, None);
    assert!(!ok && out.contains("bad.strata:1:14: this comment would be lost"), "{out}");

    // editing a comment changes the definition, but not its interface
    let hash = |file: &str| {
        let (_, out) = strata(&["--file", file, "ast", "Point"], &d, None);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        (v["hash"].as_str().unwrap().to_string(), v["interface_hash"].as_str().unwrap().to_string())
    };
    std::fs::write(d.join("c2.strata"), src.replace("points down", "points up")).unwrap();
    let (a, b) = (hash("c.strata"), hash("c2.strata"));
    assert_ne!(a.0, b.0);
    assert_eq!(a.1, b.1);
}

#[test]
fn rename_updates_every_reference() {
    let d = tmp("rename");
    let a = r#"/// The size of `xs`; see also size_of.
pub fn size(xs: List[Int]) -> Int { len(xs) }
pub type Box = { items: List[Int] }
pub fn twice(xs: List[Int]) -> Int {
  let n = size(xs)
  let tally = x -> x + n
  tally(size(xs))
}
"#;
    let b = r#"use a.{size, Box}
fn count(x: Int) -> Int { x }
fn total(b: Box) -> Int { a.size(b.items) + size([]) + count(1) }
test "boxes" {
  assert total(Box { items: [1, 2] }) == 3
}
"#;
    // a package written by hand: its files are its modules
    std::fs::write(d.join("a.strata"), a).unwrap();
    std::fs::write(d.join("b.strata"), b).unwrap();
    assert!(strata(&["init"], &d, None).0);

    // names that a local or another definition would capture are refused
    let (ok, out) = strata(&["rename", "a.size", "count"], &d, None);
    assert!(!ok && out.contains("would change what other names refer to"), "{out}");
    let (ok, out) = strata(&["rename", "size", "tally"], &d, None);
    assert!(!ok && out.contains("would change what other names refer to"), "{out}");
    let (ok, out) = strata(&["rename", "Box", "crate"], &d, None);
    assert!(!ok && out.contains("uppercase"), "{out}");

    let (ok, out) = strata(&["rename", "size", "length"], &d, None);
    assert!(ok && out.contains("renamed a.size → a.length") && out.contains("a.twice") && out.contains("b.total"), "{out}");
    assert!(out.contains("docs or comments still mention `size` in: a.length"), "{out}");
    let (ok, out) = strata(&["rename", "a.Box", "Crate"], &d, None);
    assert!(ok, "{out}");
    let r = strata(&["render", "a"], &d, None).1 + &strata(&["render", "b"], &d, None).1;
    for s in ["pub fn length(xs: List[Int])", "let n = length(xs)", "tally(length(xs))", "use a.{length, Crate}", "a.length(b.items) + length([])", "fn total(b: Crate)", "Crate { items: [1, 2] }"] {
        assert!(r.contains(s), "missing {s:?} in\n{r}");
    }
    let (ok, out) = strata(&["test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");
    let (_, log) = strata(&["log", "--json"], &d, None);
    assert!(log.contains(r#""from":"a.size""#) && log.contains(r#""to":"a.length""#), "{log}");
    // the files are the source; the log is next to them, to be committed
    assert!(std::fs::read_to_string(d.join("b.strata")).unwrap().contains("fn total(b: Crate)"));
    assert_eq!(std::fs::read_to_string(d.join("strata.log")).unwrap().lines().count(), 2);
    // the log replays backwards from the files: the first step starts from what was written by hand
    let (ok, out) = strata(&["diff", "--from", "0", "--step", "0"], &d, None);
    assert!(ok && out.contains("↪ a.length  (renamed from `size`)") && out.contains("3 references updated") && !out.contains("a.Crate  (renamed"), "{out}");
}

/// Run the `test` blocks of `src` and check that it round-trips through `render`.
fn run_tests(name: &str, src: &str, passed: &str) {
    let d = tmp(&format!("lang-{name}"));
    std::fs::write(d.join("a.strata"), src).unwrap();
    let (ok, out) = strata(&["--file", "a.strata", "test"], &d, None);
    assert!(ok && out.contains(passed), "{out}");
    let (_, r1) = strata(&["--file", "a.strata", "render"], &d, None);
    std::fs::write(d.join("r1.strata"), &r1).unwrap();
    let (_, r2) = strata(&["--file", "r1.strata", "render"], &d, None);
    assert_eq!(r1, r2);
}

/// `check` on `src` fails with a message containing `msg`.
fn check_fails(name: &str, src: &str, msg: &str) {
    let d = tmp(&format!("lang-{name}"));
    std::fs::write(d.join("a.strata"), src).unwrap();
    let (ok, out) = strata(&["--file", "a.strata", "check"], &d, None);
    assert!(!ok && out.contains(msg), "{out}");
}

#[test]
fn plus_joins_strings_and_lists() {
    run_tests(
        "plus",
        r#"
fn greet(name: String) -> String { "hello " + name + "!" }
test "join" {
  assert greet("ana") == "hello ana!"
  assert [1, 2] + [3] == [1, 2, 3]
  assert len("héllo") == 5
  assert 1 + 2 == 3
}
"#,
        "1 passed",
    );
    check_fails("plusplus", "fn f() -> String { \"a\" ++ \"b\" }", "join strings and lists with `+`");
    check_fails("plusmix", "fn f() -> String { \"a\" + 1 }", "`+` operands");
    check_fails("strlen", "fn f() -> Int { str_len(\"a\") }", "str_len");
}

#[test]
fn arrow_lambdas() {
    run_tests(
        "lambdas",
        r#"
fn apply(f: (Int) -> Int, x: Int) -> Int { f(x) }
test "arrows" {
  assert [1, 2, 3].map(x -> x * 2) == [2, 4, 6]
  assert fold([1, 2, 3], 0, (acc, x) -> acc + x) == 6
  assert apply((n: Int) -> n + 1, 1) == 2
  let k = () -> 7
  assert k() == 7
  let f = x -> {
    let y = x + 1
    y * y
  }
  assert f(2) == 9
  assert (1, x -> x).0 == 1
}
"#,
        "1 passed",
    );
    check_fails("oldlambda", "fn f() -> List[Int] { [1].map(|x| x) }", "lambdas are written `x -> body`");
    check_fails("lambdatype", "fn f() -> List[Int] { [1].map((s: String) -> 1) }", "lambda parameter `s`");
}

#[test]
fn return_propagation_and_except() {
    run_tests(
        "errors",
        r#"type ParseError =
  | Empty
  | NotANumber(String)

fn parse(s: String) -> Result[Int, ParseError] {
  if s == "" { return Err(Empty) }
  match parse_int(s) {
    Some(n) => Ok(n),
    None => Err(NotANumber(s)),
  }
}

fn sum_two(a: String, b: String) -> Result[Int, ParseError] {
  let x = parse(a)!
  Ok(x + parse(b)!)
}

fn first_even(xs: List[Int]) -> Option[Int] {
  let x = xs.find(x -> x % 2 == 0)?
  Some(x * 10)
}

fn or_zero(s: String) -> Int { parse(s) except { 0 } }

fn describe(s: String) -> String {
  let n = parse(s) except e {
    return match e {
      Empty => "empty",
      NotANumber(t) => "not a number: " + t,
    }
  }
  "the number " + to_string(n)
}

fn first_negative(xs: List[Int]) -> Int {
  for x in xs {
    if x < 0 { return x }
  }
  0
}

test "errors" {
  assert sum_two("1", "2") == Ok(3)
  assert sum_two("1", "x") == Err(NotANumber("x"))
  assert sum_two("", "x") == Err(Empty)
  assert first_even([1, 4, 5]) == Some(40)
  assert first_even([1, 3]) == None
  assert or_zero("12") == 12
  assert or_zero("zz") == 0
  assert first_negative([3, -2, -5]) == -2
  assert first_negative([3]) == 0
  let f = x -> {
    if x > 1 { return "big" }
    "small"
  }
  assert describe("7") == "the number 7"
  assert describe("q") == "not a number: q"
  assert describe("") == "empty"
  assert f(5) == "big" && f(0) == "small"
  assert [1, 2, 3].find(x -> x > 5) except { 9 } == 9
}
"#,
        "1 passed",
    );
    let pre = "fn p(s: String) -> Result[Int, String] { Err(s) }\n";
    check_fails("bang", &format!("{pre}fn f() -> Int {{ p(\"a\")! }}"), "`!` returns the error early, so the result type must be `Result[");
    check_fails("bangerr", &format!("{pre}fn f() -> Result[Int, Int] {{ Ok(p(\"a\")!) }}"), "`!` returns the error early");
    check_fails("qonresult", &format!("{pre}fn f() -> Option[Int] {{ Some(p(\"a\")?) }}"), "`?` needs an Option");
    check_fails("exceptname", "fn f() -> Int { [1].find(x -> x > 0) except e { 0 } }", "an Option has no error to name");
    check_fails("exceptint", "fn f() -> Int { 1 except { 0 } }", "`except` handles a Result or an Option");
    check_fails("returntest", "test \"t\" { return 1 }", "`return` outside a function");
    check_fails("returntype", "fn f() -> Int { return \"a\" }", "`return` value");
}

#[test]
fn f_string_interpolation() {
    run_tests(
        "fstrings",
        r##"type Color =
  | Red
  | Rgb(Int, Int, Int)

fn report(name: String, xs: List[Int]) -> String { f"{name} has {len(xs)} items" }

test "fstrings" {
  assert report("ana", [1, 2]) == "ana has 2 items"
  let n = 3
  assert f"{n} + 1 = {n + 1}" == "3 + 1 = 4"
  assert f"{{literal}} {n}" == "{literal} 3"
  assert f"quote \" and {join(["a", "b"], "-")}" == "quote \" and a-b"
  assert f"{Red} {[1, 2]} {Some(1)}" == "Red [1,2] {\"Some\":1}"
  assert f"no parts" == "no parts"
  assert f"{if n > 2 { "big" } else { "small" }}" == "big"
}
"##,
        "1 passed",
    );
    check_fails("fempty", "fn f() -> String { f\"a {} b\" }", "empty `{}` in an f-string");
    check_fails("fbrace", "fn f() -> String { f\"a } b\" }", "a lone `}` in an f-string");
    check_fails("fbad", "fn f() -> String { f\"{1 +}\" }", "in f-string");
    check_fails("fvar", "fn f() -> String { f\"{nope}\" }", "unknown variable `nope`");
}

#[test]
fn string_primitives() {
    run_tests(
        "strings",
        r##"fn count_digits(s: String) -> Int { s.chars().count(c -> is_digit(c)) }

test "strings" {
  let s = "héllo wörld"
  assert len(s) == 11
  assert s.chars().take(2) == ["h", "é"]
  assert s.get(1) == Some("é") && s.get(99) == None
  assert s.slice(6, 11) == "wörld" && s.slice(-3, 2) == "hé" && s.slice(8, 3) == ""
  assert [1, 2, 3, 4].slice(1, 3) == [2, 3]
  assert s.index_of("wö") == Some(6) && s.index_of("z") == None
  assert s.ends_with("rld") && s.starts_with("hé")
  assert s.replace("l", "L") == "héLLo wörLd"
  assert "a\nb\n".lines() == ["a", "b"]
  assert "ab".repeat(3) == "ababab"
  assert "abc".reverse() == "cba"
  assert "Ab".to_upper() == "AB" && "Ab".to_lower() == "ab"
  assert is_digit("42") && !is_digit("4a") && !is_digit("")
  assert is_alpha("é") && is_space(" \t")
  assert ord("A") == 65 && chr(233) == "é"
  assert count_digits("a1b22") == 3
  assert "a,b".split(",") == ["a", "b"] && contains("abc", "bc")
}
"##,
        "1 passed",
    );
}

#[test]
fn diff_shows_changes_structurally() {
    let d = tmp("diff");
    assert!(strata(&["init"], &d, None).0);
    assert!(strata(&["add", &example()], &d, None).0);
    assert!(strata(&["rename", "line_total", "line_cents"], &d, None).0);
    let (ok, out) = strata(&["edit", "-m", "stronger discount"], &d, Some("/// 10% off orders of 100.00 or more, never below zero.\nfn discount(amount: Cents) -> Cents\n  ensures it <= amount\n  ensures it >= 0 {\n  if amount >= 10000 { amount - amount / 10 } else { amount }\n}\n"));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["edit", "-m", "a new error"], &d, Some("type ShipError =\n  | NotFound\n  | NotPending\n  | Locked\n"));
    assert!(ok, "{out}");

    let (ok, out) = strata(&["diff", "--json"], &d, None);
    assert!(ok, "{out}");
    let j: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(j["steps"].as_array().unwrap().len(), 3, "{out}");
    let items: Vec<&serde_json::Value> = j["sections"].as_array().unwrap().iter().flat_map(|s| s["items"].as_array().unwrap()).collect();
    let item = |id: &str| *items.iter().find(|i| i["id"] == id).unwrap_or_else(|| panic!("no change for {id} in {out}"));

    // the rename is one change, and what only follows it is folded into it
    let r = item("line_cents");
    assert_eq!(r["mk"], "ren");
    assert!(r["refs"].as_array().unwrap().iter().any(|x| x == "total"), "{r}");
    assert!(!items.iter().any(|i| i["id"] == "total"), "a reference that only follows the rename is not its own change");

    // a new ensures is a stronger guarantee; the doc is a word diff
    let dc = item("discount");
    assert!(dc["what"].as_array().unwrap().iter().any(|w| w == "stronger guarantee"), "{dc}");
    assert!(dc["lines"].as_array().unwrap().iter().any(|l| l["s"] == "+" && l["note"] == "stronger guarantee" && l["t"].as_str().unwrap().contains("ensures it >= 0")), "{dc}");
    assert!(dc["doc_diff"].as_array().unwrap().iter().any(|s| s[0] == "+" && s[1].as_str().unwrap().contains("never below zero")), "{dc}");

    // a new variant, with the unchanged ones folded
    let se = item("ShipError");
    assert!(se["what"][0].as_str().unwrap().contains("variant `Locked` added"), "{se}");
    assert!(se["lines"].as_array().unwrap().iter().any(|l| l["s"] == "fold" && l["t"].as_str().unwrap().contains("NotFound and NotPending unchanged")), "{se}");

    // one step on its own, and the text form
    let (ok, out) = strata(&["diff", "--step", "1"], &d, None);
    assert!(ok && out.contains("↪ line_cents") && !out.contains("ShipError"), "{out}");
    let (ok, out) = strata(&["diff"], &d, None);
    assert!(ok && out.contains("◀ stronger guarantee") && out.contains("{+, never below zero+}"), "{out}");
}

#[test]
fn unions_variants_as_types_and_narrowing() {
    let d = tmp("unions");
    std::fs::write(d.join("u.strata"), UNIONS).unwrap();
    std::fs::write(d.join("t.strata"), TRAITS_PATHS).unwrap();
    for f in ["u.strata", "t.strata"] {
        let (ok, out) = strata(&["--file", f, "test"], &d, None);
        assert!(ok && out.contains("1 passed"), "{out}");
        let (ok, r1) = strata(&["--file", f, "render"], &d, None);
        assert!(ok, "{r1}");
        std::fs::write(d.join("r.strata"), &r1).unwrap();
        let (_, r2) = strata(&["--file", "r.strata", "render"], &d, None);
        assert_eq!(r1, r2);
    }
    // JSON input picks the union member it is a valid value of
    let (ok, out) = strata(&["--file", "u.strata", "run", "area", r#"[{"radius": 2}]"#], &d, None);
    assert!(ok && out.trim() == "12", "{out}");
    let (ok, out) = strata(&["--file", "u.strata", "run", "max_of", r#"["NonEmptyErr"]"#], &d, None);
    assert!(!ok && out.contains("expected a `TooBig`"), "{out}");

    std::fs::write(d.join("bad.strata"), BAD).unwrap();
    let (ok, out) = strata(&["--file", "bad.strata", "check"], &d, None);
    assert!(!ok);
    for e in [
        "non-exhaustive match on `Int`: `where 3 <= it && it <= 5` is not covered",
        "non-exhaustive match on `Int | String`: `String` is not covered",
        "unreachable match arm `Positive`",
        "unreachable match arm `7`",
        "pattern `String`: expected `Circle | Square`, found `String`",
        "non-exhaustive match on `String`: `where len(it) == 0` is not covered",
        "not every `Circle | Square` does",
    ] {
        assert!(out.contains(e), "missing {e:?} in\n{out}");
    }
    assert!(!out.contains("unreachable match arm `String`"), "{out}");

    // a variant is a type, so its name is unique across the project
    std::fs::create_dir_all(d.join("clash")).unwrap();
    std::fs::write(d.join("clash/a.strata"), "pub type Status = | Done | Open\n").unwrap();
    std::fs::write(d.join("clash/b.strata"), "pub type Outcome = | Done | Failed\n").unwrap();
    let (ok, out) = strata(&["--file", "clash", "check"], &d, None);
    assert!(!ok && out.contains("variant `a.Done` of `a.Status` has the same name as the variant `b.Done` of `b.Outcome`"), "{out}");
}

const UNIONS: &str = r#"type Square = { side: Positive }
type Circle = { radius: Positive }
type Triangle = { base: Positive, height: Positive }
type Shape = Square | Circle | Triangle

type ShapeError = | NonEmptyErr | TooBig { max: Int }

fn area(s: Shape) -> Int {
  match s {
    Square => s.side * s.side,
    Circle => 3 * s.radius * s.radius,
    Triangle => s.base * s.height / 2,
  }
}

fn size(n: Int) -> String {
  match n {
    where it < 3 => "small",
    where 3 <= it && it <= 5 => "medium",
    where it > 5 => "large",
  }
}

fn sign(x: Int) -> String {
  match x {
    Positive => "positive",
    0 => "zero",
    where it < 0 => "negative",
  }
}

fn describe(v: Int | String) -> String {
  match v {
    Int => f"number {v + 1}",
    String => f"text {len(v)}",
  }
}

fn check(s: Shape) -> Result[Shape, ShapeError] {
  if area(s) > 100 { Err(TooBig { max: 100 }) } else { Ok(s) }
}

fn max_of(e: TooBig) -> Int { e.max }

fn explain(r: Result[Shape, ShapeError]) -> String {
  match r {
    Ok(s) => f"ok {area(s)}",
    Err(e) => match e {
      NonEmptyErr => "empty",
      TooBig => f"too big, max {max_of(e)}",
    },
  }
}

fn first_word(s: String) -> String {
  match lines(s) as ls {
    [] => "",
    _ => match get(ls, 0) { Some(w) => w, None => "" },
  }
}

test "unions and narrowing" {
  assert area(Square { side: 3 }) == 9
  assert area(Circle { radius: 2 }) == 12
  assert size(4) == "medium"
  assert size(-10) == "small"
  assert sign(-2) == "negative"
  assert sign(0) == "zero"
  assert describe(41) == "number 42"
  assert describe("abc") == "text 3"
  assert explain(check(Square { side: 20 })) == "too big, max 100"
  assert explain(check(Circle { radius: 1 })) == "ok 3"
  assert Square { side: 1 } is Square
  assert 5 is Positive
  assert !(-5 is Positive)
}
"#;

const TRAITS_PATHS: &str = r#"trait Named { fn name(self) -> String }
type Dog = { n: String }
type Cat = { n: String }
type Status = | Open | Closed { why: String }
type Ticket = { id: Int, status: Status }
impl Named for Dog { fn name(self) -> String { "dog " + self.n } }
impl Named for Cat { fn name(self) -> String { "cat " + self.n } }
impl Named for Closed { fn name(self) -> String { "closed: " + self.why } }

fn greet(p: Dog | Cat) -> String { p.name() }
fn label(t: Ticket) -> String {
  match t.status {
    Open => "open",
    Closed => t.status.why,
  }
}
fn closed_name(c: Closed) -> String { c.name() }
fn reset(n: Int) -> Int {
  let mut x = n
  match x {
    Positive => {
      x = -1
      x
    }
    _ => 0,
  }
}
fn maybe(n: Int) -> Int | () {
  if n > 0 { n } else { () }
}
fn use_maybe(n: Int) -> Int {
  match maybe(n) as m {
    Int => m,
    Unit => 0,
  }
}
test "traits, paths, mut" {
  assert greet(Cat { n: "tom" }) == "cat tom"
  assert label(Ticket { id: 1, status: Closed { why: "done" } }) == "done"
  assert closed_name(Closed { why: "x" }) == "closed: x"
  assert reset(5) == -1
  assert use_maybe(3) == 3
  assert use_maybe(-3) == 0
}
"#;

const BAD: &str = r#"type Square = { side: Positive }
type Circle = { radius: Positive }
type Shape = Square | Circle

fn gap(n: Int) -> String {
  match n {
    where it < 3 => "small",
    where it > 5 => "large",
  }
}

fn missing_member(v: Int | String) -> String {
  match v {
    Int => "n",
  }
}

fn dead(n: Int) -> String {
  match n {
    Int => "any",
    Positive => "never",
  }
}

fn dead2(n: Int) -> String {
  match n {
    where it >= 0 => "a",
    where it < 0 => "b",
    7 => "c",
  }
}

fn wrong(s: Shape) -> Int {
  match s {
    String => 1,
    _ => 2,
  }
}

fn opaque(s: String) -> Int {
  match s {
    NonEmptyString => 1,
  }
}

fn no_field(s: Shape) -> Int { s.side }

fn mixed() -> Int {
  let x = if true { Square { side: 1 } } else { Circle { radius: 1 } }
  1
}
"#;

#[test]
fn or_patterns_refined_scrutinees_lengths_floats_and_variance() {
    let d = tmp("patterns2");
    for (f, src) in [("or.strata", OR_PATTERNS), ("dom.strata", DOMAINS)] {
        std::fs::write(d.join(f), src).unwrap();
        let (ok, out) = strata(&["--file", f, "test"], &d, None);
        assert!(ok && out.contains("1 passed"), "{out}");
        let (ok, r1) = strata(&["--file", f, "render"], &d, None);
        assert!(ok, "{r1}");
        std::fs::write(d.join("r.strata"), &r1).unwrap();
        let (_, r2) = strata(&["--file", "r.strata", "render"], &d, None);
        assert_eq!(r1, r2);
    }
    let check = |name: &str, src: &str| {
        std::fs::write(d.join(name), src).unwrap();
        strata(&["--file", name, "check"], &d, None)
    };
    let (ok, out) = check("bad.strata", DOMAINS_BAD);
    assert!(!ok);
    for e in [
        "non-exhaustive match on `Positive`: `2` is not covered",
        "unreachable match arm `where it <= 0`: no `Positive` matches it",
        "non-exhaustive match on `Float`: `where it == 0.0` is not covered",
        "non-exhaustive match on `String`: `where len(it) <= 3` is not covered",
    ] {
        assert!(out.contains(e), "missing {e:?} in\n{out}");
    }
    // a declared type holds for a variable's whole life, mutable or not
    let (ok, out) = check("mut.strata", MUT_REFINED);
    assert!(!ok);
    let errors: Vec<&str> = out.lines().filter(|l| l.starts_with("error")).collect();
    assert_eq!(errors.len(), 2, "{out}");
    assert!(out.contains("in lit: refinement violated at compile time in assigning `x`: -1 does not satisfy `Positive`"), "{out}");
    assert!(out.contains("in wb: call to `set_neg`, argument 1: `p` is declared `Positive`, but the `mut` parameter is `Int`"), "{out}");
    std::fs::write(d.join("mut_run.strata"), "fn a(mut n: Positive) -> Int {\n  n = n - 10\n  n\n}\n").unwrap();
    let (ok, out) = strata(&["--file", "mut_run.strata", "run", "a", "[3]"], &d, None);
    assert!(!ok && out.contains("assigning `n`: -7 does not satisfy `Positive`"), "{out}");
    let (ok, out) = check("or_bad.strata", "type Light = | Red | Amber | Green\nfn a(l: Light) -> Bool { match l { Red | Amber => false } }\nfn b(p: (Int, Int)) -> Int { match p { (0, x) | (y, 0) => 1, _ => 2 } }\n");
    assert!(!ok && out.contains("`Green` is not covered") && out.contains("must bind the same names"), "{out}");
    let (ok, out) = check("variance.strata", VARIANCE);
    assert!(!ok, "{out}");
    let errors: Vec<&str> = out.lines().filter(|l| l.starts_with("error")).collect();
    assert_eq!(errors.len(), 1, "{out}");
    assert!(errors[0].contains("in bad_mut") && errors[0].contains("expected exactly `List[Circle | Square]`, found `List[Circle]`"), "{out}");
}

const OR_PATTERNS: &str = r#"type Square = { side: Positive }
type Circle = { radius: Positive }
type Triangle = { base: Positive, height: Positive }
type Shape = Square | Circle | Triangle
type Light = | Red | Amber | Green

fn round(s: Shape) -> Bool {
  match s {
    Circle => true,
    Square | Triangle => false,
  }
}
fn go(l: Light) -> Bool {
  match l { Green => true, Red | Amber => false }
}
fn small(n: Int) -> String {
  match n { 0 | 1 | 2 => "tiny", _ => "big" }
}
fn pair_val(p: (Int, Int)) -> Int {
  match p { (0, x) | (x, 0) => x, (a, b) => a * b }
}
test "or" {
  assert round(Circle { radius: 1 })
  assert !round(Square { side: 2 })
  assert go(Green) && !go(Red)
  assert small(2) == "tiny" && small(3) == "big"
  assert pair_val((0, 5)) == 5 && pair_val((4, 0)) == 4 && pair_val((2, 3)) == 6
}
"#;

const DOMAINS: &str = r#"type Chance = Float where 0.0 <= it && it <= 1.0

fn f(n: Positive) -> String {
  match n { 1 => "one", where it > 1 => "more" }
}
fn head(xs: NonEmpty[Int]) -> Int {
  match xs { [x, ..] => x }
}
fn word(s: String) -> String {
  match s { where len(it) == 0 => "empty", where len(it) > 0 => "some" }
}
fn ne(s: NonEmptyString) -> String {
  match s { where len(it) == 1 => "char", where len(it) >= 2 => "word" }
}
fn temp(x: Float) -> String {
  match x { where it < 0.0 => "freezing", where 0.0 <= it && it < 20.0 => "mild", where it >= 20.0 => "warm" }
}
fn odds(p: Chance) -> String {
  match p { where it < 0.5 => "unlikely", where it >= 0.5 => "likely" }
}
fn nested(n: Int) -> String {
  match n {
    Positive => match n { 1 => "one", where it >= 2 => "many" },
    _ => "none",
  }
}
fn of_result() -> String {
  match five() { 5 => "five" }
}
fn five() -> Int where it == 5 { 5 }
fn lst(xs: List[Int]) -> Int {
  match xs { [] => 0, where len(it) > 0 => 1 }
}
test "domains" {
  assert f(1) == "one" && f(7) == "more"
  assert head([4, 5]) == 4
  assert word("") == "empty" && ne("ab") == "word"
  assert temp(25.0) == "warm" && temp(5.5) == "mild" && temp(-1.0) == "freezing"
  assert odds(0.7) == "likely"
  assert nested(3) == "many"
  assert lst([1]) == 1
}
"#;

const DOMAINS_BAD: &str = r#"fn f(n: Positive) -> String {
  match n { 1 => "one", where it > 2 => "more" }
}
fn g(n: Positive) -> String {
  match n { where it <= 0 => "never", _ => "x" }
}
fn temp(x: Float) -> String {
  match x { where it < 0.0 => "a", where it > 0.0 => "b" }
}
fn word(s: String) -> String {
  match s { where len(it) > 3 => "long" }
}
fn m(mut n: Positive) -> String {
  match n { 1 => "one", where it > 1 => "more" }
}
"#;

const VARIANCE: &str = r#"type Square = { side: Positive }
type Circle = { radius: Positive }
type Shape = Square | Circle
type Err1 = | TooBig | TooSmall

fn total(shapes: List[Shape]) -> Int { len(shapes) }
fn grow(mut shapes: List[Shape]) -> () { shapes = shapes + [Square { side: 1 }] }
fn mixed(big: Bool) -> Int {
  let s = if big { Square { side: 2 } } else { Circle { radius: 1 } }
  let xs = [Square { side: 1 }, Circle { radius: 1 }]
  total(xs)
}
fn ok_list() -> Int {
  let cs = [Circle { radius: 1 }]
  total(cs)
}
fn bad_mut() -> Int {
  let mut cs = [Circle { radius: 1 }]
  grow(mut cs)
  len(cs)
}
fn inner() -> Result[Int, TooBig] { Err(TooBig) }
fn outer() -> Result[Int, Err1] {
  let x = inner()!
  Ok(x)
}
fn apply(f: (Circle) -> Shape) -> Int { 1 }
fn id_shape(s: Shape) -> Circle { Circle { radius: 1 } }
fn use_apply() -> Int { apply(id_shape) }
"#;

const MUT_REFINED: &str = r#"fn m(mut n: Positive) -> String {
  match n { 1 => "one", where it > 1 => "more" }
}
fn changes_in_arm(mut n: Positive) -> String {
  match n {
    where it > 5 => {
      n = n - 1
      match n { where it > 4 => "big", 1 | 2 | 3 | 4 => "small" }
    },
    _ => "tiny",
  }
}
fn lit() -> Int {
  let mut x: Positive = 3
  x = -1
  x
}
fn set_neg(mut k: Int) -> () { k = -3 }
fn wb() -> Int {
  let mut p: Positive = 4
  set_neg(mut p)
  p
}
fn bump(mut k: Positive) -> () { k = k + 1 }
fn wb_ok() -> Int {
  let mut p: Natural = 1
  bump(mut p)
  p
}
"#;

/// Run `strata merge-file` on three texts: (exit ok, merged text, output).
fn merge3(name: &str, base: &str, ours: &str, theirs: &str) -> (bool, String, String) {
    let d = tmp(name);
    for (f, t) in [("o.strata", base), ("a.strata", ours), ("b.strata", theirs)] {
        std::fs::write(d.join(f), t).unwrap();
    }
    let (ok, out) = strata(&["merge-file", "o.strata", "a.strata", "b.strata", "a.strata"], &d, None);
    (ok, std::fs::read_to_string(d.join("a.strata")).unwrap(), out)
}

#[test]
fn merge_file_merges_each_definition_separately() {
    let base = "fn f(x: Int) -> Int {\n  x + 1\n}\n\nfn g(x: Int) -> Int {\n  x * 2\n}\n";
    // ours changes f and adds k; theirs changes g and adds h
    let ours = "fn f(x: Int) -> Int {\n  x + 10\n}\n\nfn g(x: Int) -> Int {\n  x * 2\n}\n\nfn k() -> Int {\n  f(2)\n}\n";
    let theirs = "fn f(x: Int) -> Int {\n  x + 1\n}\n\nfn g(x: Int) -> Int {\n  x * 20\n}\n\nfn h() -> Int {\n  g(1)\n}\n";
    let (ok, text, out) = merge3("merge-sep", base, ours, theirs);
    assert!(ok, "{out}\n{text}");
    assert_eq!(text, "fn f(x: Int) -> Int {\n  x + 10\n}\n\nfn g(x: Int) -> Int {\n  x * 20\n}\n\nfn h() -> Int {\n  g(1)\n}\n\nfn k() -> Int {\n  f(2)\n}\n");

    // ours deletes f, theirs changes the next line (g's signature): a conflict as text, not here
    let ours = "fn g(x: Int) -> Int {\n  x * 2\n}\n";
    let theirs = "fn f(x: Int) -> Int {\n  x + 1\n}\n\nfn g(y: Int) -> Int {\n  y * 2\n}\n";
    let (ok, text, out) = merge3("merge-adjacent", base, ours, theirs);
    assert!(ok && text == "fn g(y: Int) -> Int {\n  y * 2\n}\n", "{out}\n{text}");
    let d = tmp("merge-sep-text");
    for (f, t) in [("o", base), ("a", ours), ("b", theirs)] {
        std::fs::write(d.join(f), t).unwrap();
    }
    let plain = Command::new("git").args(["merge-file", "-p", "a", "o", "b"]).current_dir(&d).output().unwrap();
    assert_eq!(plain.status.code(), Some(1), "plain git conflicts on these");

    // one definition changed on both sides, in different places: merged line by line
    let base = "fn f(x: Int) -> Int {\n  let a = x + 1\n  let b = a * 2\n  let c = b - 3\n  c\n}\n";
    let ours = "fn f(x: Int) -> Int {\n  let a = x + 100\n  let b = a * 2\n  let c = b - 3\n  c\n}\n";
    let theirs = "fn f(x: Int) -> Int {\n  let a = x + 1\n  let b = a * 2\n  let c = b - 300\n  c\n}\n";
    let (ok, text, out) = merge3("merge-lines", base, ours, theirs);
    assert!(ok && text.contains("x + 100") && text.contains("b - 300"), "{out}\n{text}");

    // the same line changed on both sides: markers around that definition only
    let base = "fn f(x: Int) -> Int {\n  x + 1\n}\n\nfn g(x: Int) -> Int {\n  x * 2\n}\n";
    let ours = "fn f(x: Int) -> Int {\n  x + 2\n}\n\nfn g(x: Int) -> Int {\n  x * 2\n}\n";
    let theirs = "fn f(x: Int) -> Int {\n  x + 3\n}\n\nfn g(x: Int) -> Int {\n  x * 3\n}\n";
    let (ok, text, out) = merge3("merge-conflict", base, ours, theirs);
    assert!(!ok && out.contains("conflict in f"), "{out}");
    assert!(text.contains("<<<<<<< ours\n") && text.contains("||||||| base\n") && text.contains(">>>>>>> theirs"), "{text}");
    assert!(text.ends_with("fn g(x: Int) -> Int {\n  x * 3\n}\n"), "g merges cleanly: {text}");

    // changed on one side, deleted on the other
    let (ok, _, out) = merge3("merge-delete", base, "fn g(x: Int) -> Int {\n  x * 2\n}\n", ours);
    assert!(!ok && out.contains("f (deleted in ours, changed in theirs)"), "{out}");
}

#[test]
fn merge_file_follows_renames_and_type_checks() {
    let base = "fn area(w: Int, h: Int) -> Int {\n  w * h\n}\n\nfn square(s: Int) -> Int {\n  area(s, s)\n}\n";
    // ours renames `area` (as `strata rename` would); theirs adds a caller of the old name
    let ours = "fn rect_area(w: Int, h: Int) -> Int {\n  w * h\n}\n\nfn square(s: Int) -> Int {\n  rect_area(s, s)\n}\n";
    let theirs = format!("{base}\nfn double_square(s: Int) -> Int {{\n  area(s, s) * 2\n}}\n");
    let (ok, text, out) = merge3("merge-rename", base, ours, &theirs);
    assert!(ok, "{out}\n{text}");
    assert!(out.contains("ours renamed area → rect_area; applied to theirs"), "{out}");
    assert!(text.contains("rect_area(s, s) * 2") && !text.contains(" area(") && !text.contains("fn area"), "{text}");

    // renamed and edited: recognised by the callers that follow the new name
    let ours = "/// width times height\nfn rect_area(w: Int, h: Int) -> Int {\n  h * w\n}\n\nfn square(s: Int) -> Int {\n  rect_area(s, s)\n}\n";
    let (ok, text, out) = merge3("merge-rename-edit", base, ours, &theirs);
    assert!(ok && text.contains("rect_area(s, s) * 2") && text.contains("h * w") && !text.contains(" area("), "{out}\n{text}");
    // a new definition that merely looks like a deleted one is not a rename
    let ours = "fn square(s: Int) -> Int {\n  s * s\n}\n\nfn cube(s: Int) -> Int {\n  s * s * s\n}\n";
    let (_, _, out) = merge3("merge-not-rename", base, ours, base);
    assert!(!out.contains("renamed"), "{out}");

    // each side checks on its own, but not together: ours adds a parameter, theirs a call without it
    let ours = "fn area(w: Int, h: Int, d: Int) -> Int {\n  w * h * d\n}\n\nfn square(s: Int) -> Int {\n  area(s, s, 1)\n}\n";
    let (ok, _, out) = merge3("merge-semantic", base, ours, &theirs);
    assert!(!ok && out.contains("doesn't check") && out.contains("double_square"), "{out}");

    // sides that don't parse are merged as text
    let (ok, text, out) = merge3("merge-text", "fn (\n", "fn (\nours\n", "fn (\n");
    assert!(ok && text == "fn (\nours\n" && out.contains("merging it as text"), "{out}");
}

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn strata_git(args: &[&str], dir: &std::path::Path) -> (bool, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_strata"))
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .unwrap();
    (o.status.success(), String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr))
}

#[test]
fn ci_checks_packages_scripts_and_canonical_format() {
    let d = tmp("ci");
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(d.join("shop/tools")).unwrap();
    std::fs::write(d.join("shop/strata.toml"), "name = \"shop\"\nexports = [\"orders.total\"]\n").unwrap();
    std::fs::write(d.join("shop/orders.strata"), "pub fn total(xs: List[Int]) -> Int { sum(xs) }\n").unwrap();
    std::fs::write(d.join("shop/main.strata"), "use orders.total\n\nfn run() -> Int {\n  total([1, 2])\n}\n").unwrap();
    // a package nested in another is a separate program: its `use` doesn't see shop's modules
    std::fs::write(d.join("shop/tools/strata.toml"), "name = \"tools\"\n").unwrap();
    std::fs::write(d.join("shop/tools/main.strata"), "fn one() -> Int {\n  1\n}\n").unwrap();
    // a file outside every package is a script
    std::fs::write(d.join("loose.strata"), "fn x() -> Int {\n  1\n}\n").unwrap();
    let (ok, out) = strata_git(&["ci"], &d);
    assert!(!ok && out.contains("shop/orders.strata: not in canonical format"), "{out}");
    assert!(out.contains("✓ package shop (shop): 3 definitions") && out.contains("✓ package tools (shop/tools): 1 definitions") && out.contains("✓ loose.strata"), "{out}");
    let (ok, out) = strata_git(&["ci", "--fix"], &d);
    assert!(ok, "{out}");
    let (ok, out) = strata_git(&["ci"], &d);
    assert!(ok, "{out}");

    // exports must name a module or a `pub` definition
    std::fs::write(d.join("shop/strata.toml"), "name = \"shop\"\nexports = [\"orders.totl\", \"run\"]\n").unwrap();
    let (ok, out) = strata_git(&["ci"], &d);
    assert!(!ok && out.contains("exports `orders.totl`, which is neither") && out.contains("exports `run`, which isn't `pub`"), "{out}");
    std::fs::write(d.join("shop/strata.toml"), "name = \"shop\"\nversion = 1\n").unwrap();
    let (_, out) = strata_git(&["ci"], &d);
    assert!(out.contains("unknown field `version`"), "{out}");

    // `init` names the package after its folder; the name stays when the folder moves
    let e = d.join("billing");
    std::fs::create_dir_all(&e).unwrap();
    let (ok, out) = strata(&["init"], &e, None);
    assert!(ok && std::fs::read_to_string(e.join("strata.toml")).unwrap() == "name = \"billing\"\n", "{out}");
}

#[test]
fn sync_merges_main_with_the_driver_then_checks() {
    let d = tmp("sync");
    git(&d, &["init", "-q", "-b", "main"]);
    let base = "fn f(x: Int) -> Int {\n  x + 1\n}\n\nfn g(x: Int) -> Int {\n  x * 2\n}\n";
    std::fs::write(d.join("app.strata"), base).unwrap();
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "base"]);
    git(&d, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(d.join("app.strata"), "fn g(x: Int) -> Int {\n  x * 2\n}\n").unwrap();
    git(&d, &["commit", "-q", "-am", "feature: drop f"]);
    git(&d, &["checkout", "-q", "main"]);
    std::fs::write(d.join("app.strata"), base.replace("g(x: Int) -> Int {\n  x * 2", "g(y: Int) -> Int {\n  y * 20")).unwrap();
    git(&d, &["commit", "-q", "-am", "main: g"]);
    git(&d, &["checkout", "-q", "feature"]);

    // plain git conflicts: f's deletion touches the line before g's signature
    let (ok, out) = strata_git(&["sync", "main"], &d);
    assert!(ok && out.contains("merged main") && out.contains("✓ app.strata"), "{out}");
    let merged = std::fs::read_to_string(d.join("app.strata")).unwrap();
    assert_eq!(merged, "fn g(y: Int) -> Int {\n  y * 20\n}\n");
    assert!(git(&d, &["log", "-1", "--format=%p"]).split_whitespace().count() == 2, "a merge commit");
    let (ok, out) = strata_git(&["sync", "main"], &d);
    assert!(ok && out.contains("already up to date"), "{out}");

    // a merge that doesn't check is left staged, not committed
    git(&d, &["checkout", "-q", "main"]);
    std::fs::write(d.join("app.strata"), merged.replace("fn g(y: Int)", "fn g(x: Int, y: Int)").replace("y * 20", "x * y")).unwrap();
    git(&d, &["commit", "-q", "-am", "main: g takes two"]);
    git(&d, &["checkout", "-q", "feature"]);
    std::fs::write(d.join("app.strata"), merged.clone() + "\nfn h() -> Int {\n  g(1)\n}\n").unwrap();
    git(&d, &["commit", "-q", "-am", "feature: h"]);
    let head = git(&d, &["rev-parse", "HEAD"]);
    let (ok, out) = strata_git(&["sync", "main"], &d);
    assert!(!ok && out.contains("doesn't check"), "{out}");
    assert_eq!(git(&d, &["rev-parse", "HEAD"]), head, "nothing committed");
}

#[test]
fn the_log_is_committed_and_splits_git_diffs_into_steps() {
    let d = tmp("gitlog");
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("a.strata"), "pub fn area(w: Int, h: Int) -> Int {\n  w * h\n}\n").unwrap();
    std::fs::write(d.join("b.strata"), "use a.area\n\nfn square(s: Int) -> Int {\n  area(s, s)\n}\n").unwrap();
    assert!(strata(&["init"], &d, None).0);
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "base"]);

    // one commit: a rename and an edit through strata, then a change by hand
    assert!(strata(&["rename", "area", "rect_area"], &d, None).0);
    let (ok, out) = strata(&["edit", "--module", "a", "-m", "areas are never negative"], &d, Some("pub fn rect_area(w: Int, h: Int) -> Int requires w >= 0 && h >= 0 {\n  w * h\n}\n"));
    assert!(ok, "{out}");
    let b = std::fs::read_to_string(d.join("b.strata")).unwrap();
    assert!(b.contains("rect_area(s, s)"), "the rename rewrote the other file: {b}");
    std::fs::write(d.join("b.strata"), b.replace("rect_area(s, s)\n}", "rect_area(s, s)\n}\n\nfn cube(s: Int) -> Int {\n  square(s) * s\n}")).unwrap();
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "rect areas"]);

    let (ok, out) = strata(&["--file", ".", "diff", "HEAD~1"], &d, None);
    assert!(ok, "{out}");
    let steps: Vec<&str> = out.lines().filter(|l| l.trim_start().starts_with("step ")).collect();
    assert_eq!(steps.len(), 3, "{out}");
    assert!(steps[0].contains("rename a.area → a.rect_area") && steps[1].contains("areas are never negative") && steps[2].contains("rect areas: edits outside strata"), "{out}");
    // the range knows the rename from the log, so the callers fold into it
    assert!(out.contains("a.rect_area  (renamed from `area`, signature changed, stricter precondition)") && out.contains("+ b.cube  (added)"), "{out}");
    let (ok, out) = strata(&["--file", ".", "diff", "HEAD~1", "--step", "3"], &d, None);
    assert!(ok && out.contains("b.cube") && !out.contains("rect_area  (renamed"), "{out}");

    // the log merges by union (both sides only append)
    let attrs = std::fs::read_to_string(format!("{}/.gitattributes", env!("CARGO_MANIFEST_DIR"))).unwrap();
    assert!(attrs.lines().any(|l| l.starts_with("strata.log") && l.contains("merge=union")), "{attrs}");
}

#[test]
fn sync_carries_logged_renames_into_the_other_sides_files() {
    let d = tmp("follow");
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("a.strata"), "pub fn area(w: Int, h: Int) -> Int {\n  w * h\n}\n").unwrap();
    std::fs::write(d.join("b.strata"), "use a.area\n\nfn square(s: Int) -> Int {\n  area(s, s)\n}\n").unwrap();
    assert!(strata(&["init"], &d, None).0);
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "base"]);

    // feature renames `area`; main adds a new file that calls it by its old name
    git(&d, &["checkout", "-q", "-b", "feature"]);
    assert!(strata(&["rename", "area", "rect_area"], &d, None).0);
    git(&d, &["commit", "-q", "-am", "rename area"]);
    git(&d, &["checkout", "-q", "main"]);
    std::fs::write(d.join("c.strata"), "use a.area\n\nfn cube(s: Int) -> Int {\n  area(s, s) * s\n}\n").unwrap();
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "cubes"]);
    git(&d, &["checkout", "-q", "feature"]);

    let (ok, out) = strata_git(&["sync", "main"], &d);
    assert!(ok && out.contains("merged main"), "{out}");
    assert!(out.contains("merge: theirs code follows the rename a.area → a.rect_area (c.use a, c.cube)"), "{out}");
    let c = std::fs::read_to_string(d.join("c.strata")).unwrap();
    assert_eq!(c, "use a.rect_area\n\nfn cube(s: Int) -> Int {\n  rect_area(s, s) * s\n}\n");
    // the fix is logged, and part of the merge commit
    let log = std::fs::read_to_string(d.join("strata.log")).unwrap();
    assert!(log.contains("\"follows\""), "{log}");
    assert!(git(&d, &["status", "--porcelain"]).trim().is_empty(), "everything committed");
    assert!(git(&d, &["log", "-1", "--format=%p"]).split_whitespace().count() == 2, "a merge commit");

    // the other way round: main renames, feature's new call follows
    git(&d, &["checkout", "-q", "main"]);
    git(&d, &["merge", "-q", "feature"]);
    assert!(strata(&["rename", "rect_area", "surface"], &d, None).0);
    git(&d, &["commit", "-q", "-am", "rename rect_area"]);
    git(&d, &["checkout", "-q", "feature"]);
    std::fs::write(d.join("b.strata"), std::fs::read_to_string(d.join("b.strata")).unwrap() + "\nfn twice(s: Int) -> Int {\n  rect_area(s, s) * 2\n}\n").unwrap();
    git(&d, &["commit", "-q", "-am", "twice"]);
    let (ok, out) = strata_git(&["sync", "main"], &d);
    assert!(ok && out.contains("merged main"), "{out}");
    let b = std::fs::read_to_string(d.join("b.strata")).unwrap();
    assert!(b.contains("surface(s, s) * 2") && !b.contains("rect_area"), "{b}");

    // a rename the other side's code can't follow (a local would capture the new name) is left alone
    git(&d, &["checkout", "-q", "main"]);
    git(&d, &["merge", "-q", "feature"]);
    assert!(strata(&["rename", "surface", "extent"], &d, None).0);
    git(&d, &["commit", "-q", "-am", "rename surface"]);
    git(&d, &["checkout", "-q", "feature"]);
    std::fs::write(d.join("b.strata"), b.clone() + "\nfn padded(s: Int) -> Int {\n  let extent = 2\n  surface(s, s) + extent\n}\n").unwrap();
    git(&d, &["commit", "-q", "-am", "padded"]);
    let head = git(&d, &["rev-parse", "HEAD"]);
    let (ok, out) = strata_git(&["sync", "main"], &d);
    assert!(!ok && out.contains("doesn't check") && out.contains("b.padded"), "{out}");
    assert!(std::fs::read_to_string(d.join("b.strata")).unwrap().contains("surface(s, s) + extent"), "not captured");
    assert_eq!(git(&d, &["rev-parse", "HEAD"]), head, "nothing committed");
}

const SHAPES: &str = "pub type Shape =\n  | Circle { radius: Int }\n  | Square(Int)\n\npub type Line = { sku: String, qty: Int }\n\npub trait Sized {\n  fn size(self) -> Int\n}\n\nimpl Sized for Line {\n  fn size(self) -> Int {\n    self.qty\n  }\n}\n";
const SHAPES_MAIN: &str = "use shapes.*\n\nfn area(s: Shape) -> Int {\n  match s {\n    Circle { radius } => 3 * radius * radius,\n    Square(n) => n * n,\n  }\n}\n\nfn bigger(l: Line) -> Line {\n  Line { ..l, qty: l.qty + 1 }\n}\n\ntest \"shapes\" {\n  assert area(Circle { radius: 1 }) == 3\n  assert Square(2) is Square\n  let l = Line { sku: \"a\", qty: 2 }\n  assert bigger(l).size() == 3\n  assert l.size() == 2\n}\n";

#[test]
fn rename_variants_methods_and_fields() {
    let d = tmp("members");
    std::fs::write(d.join("shapes.strata"), SHAPES).unwrap();
    std::fs::write(d.join("main.strata"), SHAPES_MAIN).unwrap();
    assert!(strata(&["init"], &d, None).0);

    // a variant: constructors, patterns and `is`
    let (ok, out) = strata(&["rename", "Circle", "Round"], &d, None);
    assert!(ok && out.contains("renamed shapes.Circle → shapes.Round") && out.contains("updated 2 definitions that refer to it: area, test \"shapes\""), "{out}");
    // a trait method: the trait, its impls and the calls
    let (ok, out) = strata(&["rename", "size", "measure"], &d, None);
    assert!(ok && out.contains("shapes.impl Sized for Line"), "{out}");
    // a field: the declaration, accesses, literals, updates
    let (ok, out) = strata(&["rename", "Line.qty", "quantity"], &d, None);
    assert!(ok && out.contains("renamed shapes.Line.qty → shapes.Line.quantity"), "{out}");
    let (s, m) = (std::fs::read_to_string(d.join("shapes.strata")).unwrap(), std::fs::read_to_string(d.join("main.strata")).unwrap());
    for x in ["| Round { radius: Int }", "fn measure(self) -> Int {\n    self.quantity", "quantity: Int }"] {
        assert!(s.contains(x), "missing {x:?} in\n{s}");
    }
    for x in ["Round { radius } =>", "Line { ..l, quantity: l.quantity + 1 }", "area(Round { radius: 1 })", "bigger(l).measure() == 3", "Line { sku: \"a\", quantity: 2 }"] {
        assert!(m.contains(x), "missing {x:?} in\n{m}");
    }
    let (ok, out) = strata(&["test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");

    // the diff says what was renamed, and folds the code that only follows it
    let (ok, out) = strata(&["diff", "--step", "0"], &d, None);
    assert!(ok && out.contains("~ shapes.Shape  (variant `Circle` renamed to `Round`)") && out.contains("only a name changed · 2 references updated") && !out.contains("~ area"), "{out}");
    let (_, out) = strata(&["diff", "--step", "1"], &d, None);
    assert!(out.contains("~ shapes.Sized  (method `size` renamed to `measure`)"), "{out}");
    let (_, out) = strata(&["diff", "--step", "2"], &d, None);
    assert!(out.contains("~ shapes.Line  (field `qty` renamed to `quantity`)"), "{out}");

    // refused: a taken name, a field another record also has, `.value` with distinct types around
    let (ok, out) = strata(&["rename", "Round", "Square"], &d, None);
    assert!(!ok && out.contains("`shapes.Square` already exists"), "{out}");
    std::fs::write(d.join("more.strata"), "pub type Order = { sku: String, total: Int }\n\ndistinct type Id = String\n").unwrap();
    // a field another record also has: the checker tells their accesses apart
    let (ok, out) = strata(&["rename", "Line.sku", "code"], &d, None);
    assert!(ok, "{out}");
    assert!(std::fs::read_to_string(d.join("more.strata")).unwrap().contains("pub type Order = { sku: String"), "Order keeps its field");
    // `value`, which distinct types also have
    std::fs::write(d.join("more.strata"), "pub type Order = { value: Int }\n\ndistinct type Id = String\n\nfn both(o: Order, i: Id) -> String {\n  f\"{o.value} {i.value}\"\n}\n").unwrap();
    let (ok, out) = strata(&["rename", "Order.value", "amount"], &d, None);
    assert!(ok, "{out}");
    assert!(std::fs::read_to_string(d.join("more.strata")).unwrap().contains("f\"{o.amount} {i.value}\""), "the distinct type's `.value` stays");
    let (_, log) = strata(&["log", "--json"], &d, None);
    assert!(log.contains(r#""kind":"variant""#) && log.contains(r#""kind":"method""#) && log.contains(r#""kind":"field""#), "{log}");
}

#[test]
fn sync_follows_member_renames() {
    let d = tmp("follow-members");
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("shapes.strata"), SHAPES).unwrap();
    std::fs::write(d.join("main.strata"), SHAPES_MAIN).unwrap();
    assert!(strata(&["init"], &d, None).0);
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "base"]);

    // feature renames a variant and a field; main adds code that uses the old names
    git(&d, &["checkout", "-q", "-b", "feature"]);
    assert!(strata(&["rename", "Circle", "Round"], &d, None).0);
    assert!(strata(&["rename", "Line.qty", "quantity"], &d, None).0);
    git(&d, &["commit", "-q", "-am", "renames"]);
    git(&d, &["checkout", "-q", "main"]);
    std::fs::write(d.join("extra.strata"), "use shapes.*\n\nfn unit() -> Shape {\n  Circle { radius: 1 }\n}\n\nfn doubled(l: Line) -> Int {\n  l.qty * 2\n}\n").unwrap();
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "extra"]);
    git(&d, &["checkout", "-q", "feature"]);

    let (ok, out) = strata_git(&["sync", "main"], &d);
    assert!(ok && out.contains("merged main"), "{out}");
    let e = std::fs::read_to_string(d.join("extra.strata")).unwrap();
    assert!(e.contains("Round { radius: 1 }") && e.contains("l.quantity * 2"), "{e}");
}

#[test]
fn rename_a_field_that_another_record_also_has() {
    let d = tmp("shared-field");
    let src = "type Line = { sku: String, qty: Int }\n\ntype Order = { sku: String, lines: List[Line] }\n\nfn label(l: Line, o: Order) -> String {\n  l.sku + \"/\" + o.sku\n}\n\nfn first_skus(o: Order) -> List[String] {\n  o.lines.map(l -> l.sku)\n}\n\nfn fix(l: Line) -> Line {\n  match l {\n    Line { sku, .. } => Line { ..l, sku: sku + \"!\" },\n  }\n}\n\ntest \"skus\" {\n  let l = Line { sku: \"a\", qty: 1 }\n  let o = Order { sku: \"o\", lines: [l] }\n  assert label(l, o) == \"a/o\"\n  assert first_skus(o) == [\"a\"]\n  assert fix(l).sku == \"a!\"\n}\n";
    std::fs::write(d.join("main.strata"), src).unwrap();
    assert!(strata(&["init"], &d, None).0);
    // the checker says which record each `.sku` reads: Line's are renamed, Order's stay
    let (ok, out) = strata(&["rename", "Line.sku", "code"], &d, None);
    assert!(ok, "{out}");
    let m = std::fs::read_to_string(d.join("main.strata")).unwrap();
    for x in ["type Line = { code: String, qty: Int }", "type Order = { sku: String", "l.code + \"/\" + o.sku", "o.lines.map(l -> l.code)", "Line { code: sku, .. } => Line { ..l, code: sku + \"!\" }", "Line { code: \"a\", qty: 1 }", "Order { sku: \"o\"", "fix(l).code == \"a!\""] {
        assert!(m.contains(x), "missing {x:?} in\n{m}");
    }
    let (ok, out) = strata(&["test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");

    // read from a union of both records: renaming one would break it
    let e = tmp("shared-field-union");
    std::fs::write(e.join("main.strata"), "type A = { tag: String, n: Int }\n\ntype B = { tag: String }\n\nfn tag_of(x: A | B) -> String {\n  x.tag\n}\n").unwrap();
    assert!(strata(&["init"], &e, None).0);
    let (ok, out) = strata(&["rename", "A.tag", "label"], &e, None);
    assert!(!ok && out.contains("in `tag_of`, `.tag` is read from a union that also has `B.tag` (rename it too, in the same command)"), "{out}");
    // both records' fields to different names: the union access can't follow
    let (ok, out) = strata(&["rename", "A.tag", "label", "B.tag", "name"], &e, None);
    assert!(!ok && out.contains("`.tag` is read from a union whose records would get different names"), "{out}");
    // both, to the same name, in one step
    let (ok, out) = strata(&["rename", "A.tag", "label", "B.tag", "label"], &e, None);
    assert!(ok && out.contains("renamed A.tag → A.label, B.tag → B.label") && out.contains("updated 1 definition that refers to them: tag_of"), "{out}");
    let m = std::fs::read_to_string(e.join("main.strata")).unwrap();
    assert!(m.contains("type A = { label: String, n: Int }") && m.contains("type B = { label: String }") && m.contains("x.label"), "{m}");
    let log = std::fs::read_to_string(e.join("strata.log")).unwrap();
    assert_eq!(log.lines().count(), 1, "one step");
}

#[test]
fn move_a_definition_to_another_module() {
    let d = tmp("move");
    std::fs::write(d.join("orders.strata"), "pub type Order = { id: String, total: Int }\n\nfn doubled(o: Order) -> Int {\n  o.total * 2\n}\n\npub fn charge(o: Order) -> Int {\n  o.total + 1\n}\n\npub fn ship(o: Order) -> Int {\n  charge(o) + doubled(o)\n}\n").unwrap();
    std::fs::write(d.join("billing.strata"), "use orders.{Order, charge}\n\npub fn invoice(o: Order) -> Int {\n  charge(o) + orders.ship(o)\n}\n").unwrap();
    std::fs::write(d.join("main.strata"), "use orders.*\n\ntest \"charges\" {\n  let o = Order { id: \"a\", total: 1 }\n  assert charge(o) == 2\n  assert orders.charge(o) == 2\n  assert ship(o) == 4\n}\n").unwrap();
    assert!(strata(&["init"], &d, None).0);

    let (ok, out) = strata(&["move", "charge", "billing"], &d, None);
    assert!(ok && out.contains("moved orders.charge from orders to billing (billing.strata)"), "{out}");
    assert!(out.contains("updated 1 definition that refers to it: test \"charges\"") && out.contains("orders: use billing.charge") && out.contains("main: use billing.charge"), "{out}");
    let (o, b, m) = (std::fs::read_to_string(d.join("orders.strata")).unwrap(), std::fs::read_to_string(d.join("billing.strata")).unwrap(), std::fs::read_to_string(d.join("main.strata")).unwrap());
    assert!(o.starts_with("use billing.charge\n") && !o.contains("fn charge"), "{o}");
    assert!(b.starts_with("use orders.Order\n") && b.contains("pub fn charge(o: Order) -> Int"), "{b}");
    assert!(m.contains("use billing.charge") && m.contains("billing.charge(o) == 2") && !m.contains("orders.charge"), "{m}");
    let (ok, out) = strata(&["test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");

    // private things stay in reach
    let (ok, out) = strata(&["move", "ship", "billing"], &d, None);
    assert!(!ok && out.contains("`orders.ship` uses `orders.doubled`, which is private to `orders`"), "{out}");
    let (ok, out) = strata(&["move", "doubled", "billing"], &d, None);
    assert!(!ok && out.contains("`orders.doubled` is private and `orders.ship` uses it"), "{out}");

    // a move is one change in the diff
    let (ok, out) = strata(&["diff", "--step", "0"], &d, None);
    assert!(ok && out.contains("billing.charge  (moved from `orders`)") && out.contains("only its module changed"), "{out}");
    let (_, log) = strata(&["log", "--json"], &d, None);
    assert!(log.contains(r#""kind":"move""#), "{log}");
}

#[test]
fn sync_follows_moves() {
    let d = tmp("follow-move");
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("orders.strata"), "pub type Order = { id: String, total: Int }\n\npub fn charge(o: Order) -> Int {\n  o.total + 1\n}\n").unwrap();
    std::fs::write(d.join("billing.strata"), "use orders.Order\n\npub fn invoice(o: Order) -> Int {\n  o.total\n}\n").unwrap();
    assert!(strata(&["init"], &d, None).0);
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "base"]);

    // feature moves `charge`; main adds code that calls it where it was
    git(&d, &["checkout", "-q", "-b", "feature"]);
    assert!(strata(&["move", "charge", "billing"], &d, None).0);
    git(&d, &["commit", "-q", "-am", "move charge"]);
    git(&d, &["checkout", "-q", "main"]);
    std::fs::write(d.join("report.strata"), "use orders.{Order, charge}\n\nfn twice(o: Order) -> Int {\n  charge(o) + orders.charge(o)\n}\n").unwrap();
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "report"]);
    git(&d, &["checkout", "-q", "feature"]);

    let (ok, out) = strata_git(&["sync", "main"], &d);
    assert!(ok && out.contains("merged main") && out.contains("follows the rename orders.charge → billing.charge"), "{out}");
    let r = std::fs::read_to_string(d.join("report.strata")).unwrap();
    assert!(r.contains("use orders.Order") && r.contains("use billing.charge") && r.contains("charge(o) + billing.charge(o)"), "{r}");
}

#[test]
fn lambda_parameter_refinements_hold_at_runtime() {
    let d = tmp("lambda-refine");
    std::fs::write(
        d.join("l.strata"),
        "fn apply(f: (Int) -> Int, x: Int) -> Int {\n  f(x)\n}\n\nfn bad() -> Int {\n  apply((n: Positive) -> n * 2, -3)\n}\n\nfn good() -> Int {\n  apply((n: Positive) -> n * 2, 3)\n}\n",
    )
    .unwrap();
    let (ok, out) = strata(&["--file", "l.strata", "run", "good"], &d, None);
    assert!(ok && out.trim() == "6", "{out}");
    let (ok, out) = strata(&["--file", "l.strata", "run", "bad"], &d, None);
    assert!(!ok && out.contains("argument `n` of a lambda: -3 does not satisfy `Positive`"), "{out}");
}

#[test]
fn f_string_format_specs() {
    let d = tmp("fmt-spec");
    std::fs::write(
        d.join("f.strata"),
        "type Cents = Int where it >= 0\n\nfn row(name: String, price: Float, qty: Cents, delta: Int) -> String {\n  f\"[{name:<8}|{price:>8.2}|{qty:03}|{delta:+}|{name:*^10}|{name:.3}|{len([1, 2].map((n: Int) -> n))}]\"\n}\n\ntest \"specs\" {\n  assert row(\"tea\", 3.14159, 7, 5) == \"[tea     |    3.14|007|+5|***tea****|tea|2]\"\n  assert row(\"biscuits\", -2.5, 42, -3) == \"[biscuits|   -2.50|042|-3|*biscuits*|bis|2]\"\n  assert f\"{-42:06}\" == \"-00042\"\n}\n",
    )
    .unwrap();
    let (ok, out) = strata(&["--file", "f.strata", "test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");
    // specs survive rendering
    let (_, r) = strata(&["--file", "f.strata", "render"], &d, None);
    assert!(r.contains("{price:>8.2}") && r.contains("{name:*^10}") && r.contains("{qty:03}"), "{r}");
    std::fs::write(d.join("e.strata"), "fn a(n: Int) -> String { f\"{n:.2}\" }\nfn b(s: String) -> String { f\"{s:05}\" }\n").unwrap();
    let (ok, out) = strata(&["--file", "e.strata", "check"], &d, None);
    assert!(!ok && out.contains("`.N` applies to a Float") && out.contains("`+` and `0` apply to numbers"), "{out}");
    std::fs::write(d.join("x.strata"), "fn c(n: Int) -> String { f\"{n:x}\" }\n").unwrap();
    let (ok, out) = strata(&["--file", "x.strata", "check"], &d, None);
    assert!(!ok && out.contains("bad format spec `x`"), "{out}");
}
