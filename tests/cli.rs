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
fn sneaky(id: OrderId) -> Bool { Db.load(id) is Some }
fn bad_qty() -> Line { Line { sku: "x", qty: 1200, unit_price: 5 } }
"#;
    std::fs::write(d.join("bad.strata"), src).unwrap();
    let (ok, out) = strata(&["--file", "bad.strata", "check"], &d, None);
    assert!(!ok);
    assert!(out.contains("`Cancelled { .. }` is not covered"), "{out}");
    assert!(out.contains("neither declared by `sneaky`"), "{out}");
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
    let ex = format!("{}/examples/tasks/tasks.strata", env!("CARGO_MANIFEST_DIR"));
    let (ok, out) = strata(&["--file", &ex, "test"], &d, None);
    assert!(ok && out.contains("0 failed"), "{out}");
    let (ok, out) = strata(&["--file", &ex, "verify", "simulate,run_worker,mem_claim", "--cases", "300"], &d, None);
    assert!(ok, "{out}");
    let (ok, out) = strata(&["--file", &ex, "run", "demo"], &d, None);
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
    for ex in ["bank.strata", "orders.strata", "builtins.strata", "tasks/tasks.strata"] {
        let path = format!("{}/examples/{ex}", env!("CARGO_MANIFEST_DIR"));
        let (_, r1) = strata(&["--file", &path, "render"], &d, None);
        std::fs::write(d.join("r.strata"), &r1).unwrap();
        let (_, r2) = strata(&["--file", "r.strata", "render"], &d, None);
        assert_eq!(r1, r2, "{ex} does not round-trip");
        let (ok, out) = strata(&["--file", "r.strata", "test"], &d, None);
        assert!(ok, "{ex} tests fail after round-trip: {out}");
    }
}
