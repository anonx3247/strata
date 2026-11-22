use std::io::Write;
use std::process::{Command, Stdio};


/// Store a program written as text at `path` (a module file): text only goes
/// in through an edit.
fn save(path: &std::path::Path, src: impl AsRef<str>) {
    // the file is replaced, not edited
    let _ = std::fs::remove_file(path);
    let mut c = Command::new(env!("CARGO_BIN_EXE_strata"))
        .args(["--file", &path.to_string_lossy(), "edit"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(src.as_ref().as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    // stored either way: some programs here are wrong on purpose
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success() || err.contains("doesn't check"), "{}: {err}", path.display());
}

/// Store a module of the folder program `dir` (no package: its files are its
/// modules), written as text.
fn save_module(dir: &std::path::Path, module: &str, src: impl AsRef<str>) {
    let rel = if module == "main" { "main".to_string() } else { module.replace('.', "/") };
    let _ = std::fs::remove_file(dir.join(format!("{rel}.strata")));
    let (ok, out) = strata(&["--file", &format!("{}/", dir.display()), "edit", module], dir, Some(src.as_ref()));
    // stored either way: the folder may not check until its other modules are written
    assert!(ok || out.contains("doesn't check"), "{module}: {out}");
}

/// A module file, as text.
fn text(path: &std::path::Path) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_strata")).args(["render", "--file"]).arg(path).output().unwrap();
    assert!(o.status.success(), "{}: {}", path.display(), String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}

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
    save(&d.join("r1.strata"), &r1);
    let (_, r2) = strata(&["--file", "r1.strata", "render"], &d, None);
    assert_eq!(r1, r2);
}

#[test]
fn bindings_and_assignments() {
    let d = tmp("bindings");
    let f = d.join("b.strata");
    // written loosely; render prints the canonical forms. A call's `(` is
    // against the name, so `a (q, r) := …` is `a`, then a binding.
    save(&f, "fn log(s: String) -> () { print(s) }\n\
              fn f() -> Int {\n\
                n := 1\n\
                let k: Int = 2\n\
                (a, b) := (3, 4)\n\
                m := a (q, r) := (a, b)\n\
                mut x := 0\n\
                mut y: Int = 10\n\
                x += n + k\n\
                x = x * 2\n\
                y -= a - b\n\
                y %= 7\n\
                x + y\n\
              }\n\
              test \"f\" { assert f() == 10 }\n");
    let t = text(&f);
    assert!(t.contains("fn log(s: String) {"), "{t}");
    for line in ["n := 1", "let k: Int = 2", "(a, b) := (3, 4)", "m := a\n  (q, r) := (a, b)", "mut x := 0", "mut y: Int = 10", "x += n + k", "x *= 2", "y -= a - b", "y %= 7"] {
        assert!(t.contains(line), "{line}: {t}");
    }
    let (ok, out) = strata(&["--file", "b.strata", "test"], &d, None);
    assert!(ok, "{out}");
    // `let` needs a type; `:=` can't change a field
    for (src, msg) in [
        ("fn g() -> Int { let n = 1\n n }", "`let` needs a type"),
        ("type P = { a: Int }\nfn g(p: P) -> Int { p.a := 1\n 0 }", "`:=` declares a new name"),
    ] {
        let _ = std::fs::remove_file(d.join("e.strata"));
        let (ok, out) = strata(&["--file", "e.strata", "edit"], &d, Some(src));
        assert!(!ok && out.contains(msg), "{src}: {out}");
    }
}

#[test]
fn pack_a_package_into_one_file() {
    let d = tmp("pack");
    let pkg = format!("{}/examples/tasks", env!("CARGO_MANIFEST_DIR"));
    // a package is a folder of module files; `pack` makes one bundle of it
    let (ok, out) = strata(&["--file", &pkg, "pack", "tasks.strata"], &d, None);
    assert!(ok && out.contains("67 definitions"), "{out}");
    assert!(std::fs::read(d.join("tasks.strata")).unwrap().starts_with(b"STRATA"));
    // every command reads a bundle like the package
    let (ok, out) = strata(&["--file", "tasks.strata", "check"], &d, None);
    assert!(ok && out.contains("67 definitions check"), "{out}");
    let (ok, out) = strata(&["--file", "tasks.strata", "test"], &d, None);
    assert!(ok && out.contains("0 failed"), "{out}");
    // the header, read without decoding the definitions
    let (ok, out) = strata(&["header", &format!("{pkg}/core.strata")], &d, None);
    assert!(ok && out.starts_with("module core:") && out.contains("uses "), "{out}");
    // rendered back as text, module by module, it is the package's text
    let (ok, out) = strata(&["--file", "tasks.strata", "render", "--split", "out"], &d, None);
    assert!(ok, "{out}");
    for m in ["core", "memory", "print", "sim"] {
        assert_eq!(text(std::path::Path::new(&format!("{pkg}/{m}.strata"))), std::fs::read_to_string(d.join(format!("out/{m}.st"))).unwrap(), "{m}");
    }
}

#[test]
fn run_a_binary_file() {
    let d = tmp("pack-run");
    let hello = format!("{}/examples/hello", env!("CARGO_MANIFEST_DIR"));
    let (ok, out) = strata(&["--file", &hello, "pack", "hello.strata"], &d, None);
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "hello.strata"], &d, None);
    assert!(ok && out.contains("hello, world!"), "{out}");
    std::fs::write(d.join("bad.strata"), b"STRATA\x01\x00\x05").unwrap();
    let (ok, out) = strata(&["--file", "bad.strata", "check"], &d, None);
    assert!(!ok && out.contains("corrupt binary Strata file"), "{out}");
    // text is not read: it goes in through an edit
    std::fs::write(d.join("text.strata"), "fn main() { print(1) }\n").unwrap();
    let (ok, out) = strata(&["run", "text.strata"], &d, None);
    assert!(!ok && out.contains("text goes in through `strata edit`"), "{out}");
}

#[test]
fn static_errors_are_reported() {
    let d = tmp("errors");
    let src = text(std::path::Path::new(&example()))
        + r#"
fn status(o: Order) -> String { match o { Pending { .. } => "p", Shipped { .. } => "s" } }
fn sneaky(id: OrderId) -> Bool { id.load(id) is Some }
fn bad_qty() -> Line { Line { sku: "x", qty: 1200, unit_price: 5 } }
"#;
    save(&d.join("bad.strata"), src);
    let (ok, out) = strata(&["--file", "bad.strata", "check"], &d, None);
    assert!(!ok);
    assert!(out.contains("`Cancelled` is not covered"), "{out}");
    assert!(out.contains("`String` does not implement `Db`"), "{out}");
    assert!(out.contains("refinement violated at compile time"), "{out}");
}

#[test]
fn verify_finds_counterexamples() {
    let d = tmp("cex");
    let src = text(std::path::Path::new(&example()))
        + "fn promo(amount: Cents, coupon: Int) -> Cents requires coupon >= 0 { amount - coupon }\n";
    save(&d.join("bug.strata"), src);
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
    save(&d.join("b.strata"), src);
    let (ok, out) = strata(&["--file", "b.strata", "test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");
    // larger than 64 bits, written as any integer is
    let (ok, out) = strata(&["--file", "b.strata", "run", "widen", "18446744073709551614"], &d, None);
    assert!(ok && out.trim() == "18446744073709551615", "{out}");
    let (ok, out) = strata(&["--file", "b.strata", "run", "widen", "18446744073709551615"], &d, None);
    assert!(!ok && out.contains("18446744073709551616") && out.contains("does not satisfy `U64`"), "{out}");
    if has_solver() {
        let (ok, out) = strata(&["--file", "b.strata", "verify", "wrap,neg,widen", "--cases", "20"], &d, None);
        assert!(!ok && out.contains("input:   255\n") && out.contains("input:   -128\n") && out.contains("input:   18446744073709551615\n"), "sized types catch overflow: {out}");
    }
    // render keeps big literals
    let (_, r) = strata(&["--file", "b.strata", "render"], &d, None);
    assert!(r.contains("15511210043330985984000000"), "{r}");
}

#[test]
fn loops_forget_variables_changed_anywhere_in_their_body() {
    // `bump(mut n)` inside a list literal changes `n`: the loop must not keep `n == 0`
    if !has_solver() {
        return;
    }
    let d = tmp("loop-changes");
    let src = "fn bump(mut x: Int) -> Int {\n  x = x + 1\n  x\n}\n\nfn count() -> Int\n  ensures it == 0\n{\n  mut n := 0\n  mut i := 0\n  while i < 3 {\n    ys := [bump(mut n)]\n    i = i + 1\n  }\n  n\n}\n";
    save(&d.join("l.strata"), src);
    let (ok, out) = strata(&["--file", "l.strata", "verify"], &d, None);
    assert!(!ok && out.contains("✗ count") && !out.contains("bug in the prover"), "{out}");
}

#[test]
fn generators_read_disjunctive_refinements() {
    // without a solver everything is fuzzed: values of `Far` must be generated, not rejected 300 times
    let d = tmp("gen-or");
    save(&d.join("g.strata"), "type Far = Int where it > 1000000 || it < -1000000\n\nfn half(x: Far) -> Int\n  ensures it != 0\n{\n  x / 2\n}\n");
    let o = Command::new(env!("CARGO_BIN_EXE_strata")).args(["--file", "g.strata", "verify"]).env("STRATA_SOLVER", d.join("no-solver")).current_dir(&d).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(out.contains("fuzzed: 200 cases"), "{out}");
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
    save(&d.join("b.strata"), "type Quantity = Int where 1 <= it && it <= 99\nfn bump(q: Quantity) -> Quantity { q + 1 }\n");
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
fn solver_flags_follow_the_solver() {
    // fake solvers that record how they are called: cvc5 gets its own flags
    let d = tmp("solver-flags");
    save(&d.join("b.strata"), "type Quantity = Int where 1 <= it && it <= 99\nfn clamp(q: Quantity) -> Quantity { if q < 99 { q + 1 } else { q } }\n");
    for (name, version, expected) in [("cvc5", "This is cvc5 version 1.4.2", "--lang smt2 --produce-models --rlimit-per="), ("z3", "Z3 version 4.15.0 - 64 bit", "-in -T:20 rlimit=")] {
        let fake = d.join(format!("fake-{name}"));
        let log = d.join(format!("{name}.log"));
        std::fs::write(&fake, format!("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo '{version}'; exit 0; fi\necho \"$@\" >> {}\ncat > /dev/null\necho unsat\n", log.display())).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let o = Command::new(env!("CARGO_BIN_EXE_strata")).args(["--file", "b.strata", "verify", "--cross-check", "0"]).env("STRATA_SOLVER", &fake).current_dir(&d).output().unwrap();
        let out = String::from_utf8_lossy(&o.stdout);
        assert!(o.status.success() && out.contains("✓ clamp"), "{out}");
        let calls = std::fs::read_to_string(&log).unwrap();
        assert!(calls.lines().count() > 0 && calls.lines().all(|l| l.starts_with(expected)), "{name} was called as: {calls}");
    }
}

#[test]
fn package_verify_policies() {
    if !has_solver() {
        eprintln!("z3 is not installed: skipping");
        return;
    }
    let d = tmp("policy");
    std::fs::write(d.join("strata.toml"), "name = \"pol\"\n\n[verify]\nterminates = [\"*\"]\nno_recursion = [\"core\"]\nproven = [\"core\"]\n").unwrap();
    save_module(&d, "core",
        "pub fn fact(n: Natural) -> Positive { if n == 0 { 1 } else { n * fact(n - 1) } }\npub fn spin(n: Int) -> Int {\n  mut k := n\n  loop {\n    if k == 1 { break }\n    k = k - 1\n  }\n  k\n}\n");
    save_module(&d, "main", "use core.*\nfn fib(n: Natural) -> Natural { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }\n");
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
  mut i := 0
  while i < n {
    i = i + 1
  }
  i
}
fn forever(n: Int) -> Int { forever(n) }
fn spin(n: Int) -> Int {
  mut k := n
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
    mid := (lo + hi) / 2
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
    save(&d.join("t.strata"), src);
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
    save(&d.join("r.strata"), &r);
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
    save(&d.join("h.strata"), src);
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
  mut k: Natural = start
  while k > 0 {
    k = k - 1
  }
  k
}
fn bad_countdown(start: Natural) -> Int {
  mut k: Natural = start
  k = k - 1
  k
}
";
    save(&d.join("p.strata"), src);
    let (ok, out) = strata(&["--file", "p.strata", "verify", "--cases", "20"], &d, None);
    assert!(!ok, "{out}");
    for name in ["area", "grow", "small", "countdown"] {
        assert!(out.contains(&format!("✓ {name} ")), "{name} should be proven: {out}");
    }
    assert!(out.contains("✗ bad_small") && out.contains("input:   -2\n"), "{out}");
    assert!(out.contains("✗ bad_countdown") && out.contains("assigning `k`: -1 does not satisfy `Natural`"), "a declared type holds after assignment: {out}");
    // what a `mut` argument held before the call is forgotten, field or not
    let src = "type Box = { n: Int }
fn bump(mut x: Int) { x = x - 5 }
fn stale(a: Natural) -> Natural {
  mut x := a
  bump(mut x)
  x
}
fn stale_field(a: Natural) -> Natural {
  mut b := Box { n: a }
  bump(mut b.n)
  b.n
}
fn fine(a: Natural) -> Natural {
  mut x := a
  bump(mut x)
  a
}
";
    save(&d.join("m.strata"), src);
    let (ok, out) = strata(&["--file", "m.strata", "verify", "--cases", "20"], &d, None);
    assert!(!ok && out.contains("✗ stale ") && out.contains("✗ stale_field") && out.contains("✓ fine"), "{out}");
}

#[test]
fn smt_models_union_values() {
    if !has_solver() {
        eprintln!("z3 is not installed: skipping");
        return;
    }
    let d = tmp("smt-unions");
    let src = "type Square = { side: Positive }
type Circle = { radius: Positive }
type Shape = Square | Circle
type E =
  | A { x: Int }
  | B
  | C
type One = { n: Positive, a: Int }
type Two = { n: Positive }
type Holder = { s: Shape }
fn area(s: Shape) -> Positive { match s { Square => s.side * s.side, Circle => 3 * s.radius * s.radius } }
fn bad_area(s: Shape) -> Positive { match s { Square => s.side - 1, Circle => s.radius } }
fn pick(big: Bool) -> Shape { if big { Square { side: 5 } } else { Circle { radius: 1 } } }
fn pick_area(big: Bool) -> Positive { area(pick(big)) }
fn make(big: Bool) -> Int where it > 2 {
  s := if big { Square { side: 5 } } else { Circle { radius: 1 } }
  match s { Square => s.side, Circle => s.radius + 2 }
}
fn bad_make(big: Bool) -> Int where it > 2 {
  s := if big { Square { side: 5 } } else { Circle { radius: 1 } }
  match s { Square => s.side, Circle => s.radius }
}
fn describe(v: Int | String) -> Natural { match v { Int => abs(v), String => len(v.code_points()) } }
fn bad_describe(v: Int | String) -> Positive { match v { Int => abs(v) + 1, String => len(v.code_points()) } }
fn is_sq(s: Shape) -> Bool ensures it == (s is Square) { match s { Square => true, Circle => false } }
fn always_sq(s: Shape) -> Bool ensures it { s is Square }
fn lit(v: Int | String) -> Positive { match v { 0 => 1, \"a\" => 2, _ => 3 } }
fn nonzero(x: Positive | Negative) -> Int where it != 0 { x }
fn bad_nonzero(x: Int) -> Positive | Negative { x }
fn variants(v: A | B) -> Natural { match v { A => abs(v.x), B => 0 } }
fn bad_variants(v: A | C) -> Bool ensures it { v is A }
fn opt(v: Option[Natural] | String) -> Natural { match v { Some(x) => x, None => 0, String => len(v.code_points()) } }
fn bad_opt(v: Option[Int] | String) -> Natural { match v { Some(x) => x, None => 0, String => len(v.code_points()) } }
fn self_eq(a: Int | String) -> Bool ensures it { a == a }
fn bad_eq(a: Int | String, b: Int | String) -> Bool ensures it { a == b }
fn bad_field(h: Holder) -> Bool ensures it { h.s is Circle }
fn common(v: One | Two) -> Positive { v.n }
fn bad_common(v: One | Two) -> Positive { v.n - 1 }
";
    save(&d.join("u.strata"), src);
    let (ok, out) = strata(&["--file", "u.strata", "verify", "--cases", "20"], &d, None);
    assert!(!ok, "{out}");
    for name in ["area", "pick", "pick_area", "make", "describe", "is_sq", "lit", "nonzero", "variants", "opt", "self_eq", "common"] {
        assert!(out.contains(&format!("✓ {name} ")), "{name} should be proven: {out}");
    }
    for name in ["bad_area", "bad_make", "bad_describe", "always_sq", "bad_nonzero", "bad_variants", "bad_opt", "bad_eq", "bad_field", "bad_common"] {
        let line = out.lines().find(|l| l.split_whitespace().nth(1) == Some(name)).unwrap_or("");
        assert!(line.starts_with("✗") && line.contains("found by the solver"), "{name} should be refuted by the solver: {out}");
    }
    assert!(out.contains("input:   Circle { radius: 1 }\n") && out.contains("input:   C\n"), "counterexamples are union members: {out}");
}

#[test]
fn smt_models_string_builtins() {
    if !has_solver() {
        eprintln!("z3 is not installed: skipping");
        return;
    }
    let d = tmp("smt-strings");
    let src = r#"fn head(s: String, n: Natural) -> String ensures byte_len(it) <= n && byte_len(it) <= byte_len(s) { byte_slice(s, 0, n) }
fn bad_head(s: String, n: Int) -> String ensures byte_len(it) == n { byte_slice(s, 0, n) }
fn tail(s: String) -> String ensures byte_len(it) <= byte_len(s) { byte_slice(s, 1, byte_len(s)) }
fn back(s: String) -> String ensures it == "" { byte_slice(s, 5, 2) }
fn n_chars(s: String) -> Bool ensures it { len(chars(s)) == len(s.code_points()) }
fn bytes_bound(s: String) -> Bool ensures it { len(s.code_points()) <= len(s.bytes()) }
fn find(s: String, p: String) -> Int ensures it <= byte_len(s) { match index_of(s, p) { Some(i) => i + byte_len(p), None => 0 } }
fn bad_find(s: String, p: String) -> Natural ensures it < byte_len(s) { match index_of(s, p) { Some(i) => i, None => 0 } }
fn empty_found(s: String) -> Bool ensures it { index_of(s, "") == Some(0) }
fn shown(n: Int) -> String ensures n < 0 ==> starts_with(it, "-") { to_string(n) }
fn shown_len(n: Int where 10 <= it && it <= 99) -> String ensures len(it.code_points()) == 2 { to_string(n) }
fn bad_shown(n: Int) -> String ensures len(it.code_points()) <= 2 { to_string(n) }
fn parsed() -> Bool ensures it { parse_int("-17") == Some(0 - 17) && parse_int("abc") is None && parse_int("") is None }
fn round_trip(n: Int) -> Bool ensures it { parse_int(to_string(n)) == Some(n) }
fn bad_parsed(s: String) -> Bool ensures it { parse_int(s) is None }
fn spaced() -> Bool ensures it { parse_int(" 7") == Some(7) }
fn digits(s: String where is_digit(it)) -> Int ensures it >= 0 { parse_int(s) or { 0 } }
fn ends(s: String) -> Bool ensures it { ends_with(s + ".txt", ".txt") }
fn bad_ends(s: String) -> Bool ensures it { ends_with(s, ".txt") }
fn rep() -> Bool ensures it { replace("aaa", "aa", "b") == "ba" }
fn bad_rep() -> Bool ensures it { replace("aaa", "aa", "b") == "bb" }
fn empty_rep(s: String, p: String) -> String { replace(s, p, "b") }
"#;
    save(&d.join("s.strata"), src);
    let (ok, out) = strata(&["--file", "s.strata", "verify", "--cases", "20"], &d, None);
    assert!(!ok, "{out}");
    for name in ["head", "tail", "back", "n_chars", "bytes_bound", "find", "empty_found", "shown", "shown_len", "parsed", "digits", "ends", "rep"] {
        assert!(out.contains(&format!("✓ {name} ")), "{name} should be proven: {out}");
    }
    // Z3 proves the round trip through `str.from_int` / `str.to_int`; cvc5 answers unknown
    assert!(passed(&out, "round_trip"), "round_trip must not be refuted: {out}");
    for name in ["bad_head", "bad_find", "bad_shown", "bad_parsed", "bad_ends", "bad_rep"] {
        let line = out.lines().find(|l| l.split_whitespace().nth(1) == Some(name)).unwrap_or("");
        assert!(line.starts_with("✗") && line.contains("found by the solver"), "{name} should be refuted by the solver: {out}");
    }
    // leading whitespace is trimmed by the interpreter: not modelled, so not refuted either
    // `replace` is total (an empty pattern is found around every character): nothing to refute
    assert!(out.contains("~ spaced") && !out.contains("✗ empty_rep"), "{out}");
    // a big offset is clamped like any other, and one inside a character moves back to its start
    save(&d.join("b.strata"), "fn s(t: String) -> String { byte_slice(t, 0, 99999999999999999999) }\nfn m(t: String) -> String { byte_slice(t, 0, 2) }\n");
    let (ok, out) = strata(&["--file", "b.strata", "run", "s", "\"abc\""], &d, None);
    assert!(ok && out.contains("\"abc\""), "{out}");
    let (ok, out) = strata(&["--file", "b.strata", "run", "m", "\"aé\""], &d, None);
    assert!(ok && out.contains("\"a\""), "{out}");
}

#[test]
fn smt_translates_unicode_regexes() {
    if !has_solver() {
        eprintln!("z3 is not installed: skipping");
        return;
    }
    let d = tmp("smt-regex");
    let src = r#"fn f(s: String where matches(it, "\d{3}")) -> String where matches(it, "\d+") { s }
fn bad_f(s: String where matches(it, "\d+")) -> String where matches(it, "\d{3}") { s }
fn word(s: String where matches(it, "[a-z]+")) -> String where matches(it, "\w+") { s }
fn bad_word(s: String where matches(it, "\w+")) -> String where matches(it, "[a-z]+") { s }
fn letters(s: String where matches(it, "\p{L}+")) -> String where matches(it, "\w+") { s }
fn bad_letters(s: String where matches(it, "\w+")) -> String where matches(it, "\p{L}+") { s }
fn spaces(s: String where matches(it, "\s*")) -> Bool ensures it { !matches(s, ".*\w.*") }
fn nbsp() -> Bool ensures it { matches(" ", "\s") }
fn caseless(s: String where matches(it, "(?i)abc")) -> Bool ensures it { len(s.code_points()) == 3 }
fn kelvin() -> Bool ensures it { matches("K", "(?i)k") }
fn bad_caseless(s: String where matches(it, "(?i)abc")) -> Bool ensures it { s == "abc" }
fn arabic() -> Bool ensures it { matches("٣", "\d") }
fn ascii_only(s: String where matches(it, "\d")) -> Bool ensures it { matches(s, "[0-9]") }
fn anchored(s: String where matches(it, "^ab$")) -> Bool ensures it { s == "ab" }
fn far() -> Bool ensures it { len("TAG".code_points()) == 9 }
"#
    .replace("TAG", "\u{E0041}");
    save(&d.join("r.strata"), src);
    let (ok, out) = strata(&["--file", "r.strata", "verify", "--cases", "20"], &d, None);
    assert!(!ok, "{out}");
    for name in ["f", "word", "letters", "spaces", "nbsp", "caseless", "kelvin", "arabic", "anchored"] {
        assert!(out.contains(&format!("✓ {name} ")), "{name} should be proven: {out}");
    }
    let line = |name: &str| out.lines().find(|l| l.split_whitespace().nth(1) == Some(name)).unwrap_or("").to_string();
    // with an approximated class (\w, \p{L}, Unicode \d) the solver's model may lie in the
    // approximation's slack: replaying it then fails the precondition, and the fuzzer finds a
    // real counterexample. Which one reports it depends on the model the solver returns.
    for name in ["bad_f", "bad_word", "bad_letters", "ascii_only"] {
        assert!(line(name).starts_with("✗"), "{name} should be refuted: {out}");
    }
    // exact models: every model the solver gives is a real counterexample
    for name in ["bad_caseless", "far"] {
        assert!(line(name).starts_with("✗") && line(name).contains("found by the solver"), "{name} should be refuted by the solver: {out}");
    }
    // a character past U+2FFFF is no SMT character: the literal is unknown, not 9 characters
    assert!(out.contains("✗ far") && !out.contains("bug in the prover"), "{out}");
}

/// Builtins that fault on some arguments are checks: proven where the
/// arguments allow it, refuted with an input where they don't; float
/// arithmetic (not modelled) is never assumed finite.
#[test]
fn builtins_that_fault_are_checked() {
    if !has_solver() {
        eprintln!("z3 is not installed: skipping");
        return;
    }
    let d = tmp("builtin-faults");
    let src = r#"fn bad_chr(n: Int) -> String { chr(n) }
fn good_chr(n: Int where 97 <= it && it <= 122) -> String ensures len(it.code_points()) == 1 { chr(n) }
fn bad_ord(s: String) -> Int { ord(s) }
fn good_ord(s: String) -> Int { if len(s.code_points()) == 1 { ord(s) } else { 0 } }
fn each_ord(s: String) -> Int { chars(s).fold(0, (a, c) -> a + ord(c)) }
fn round_trip() -> Bool ensures it { ord(chr(955)) == 955 }
fn any_split(s: String, sep: String) -> Int { len(split(s, sep)) }
fn any_repeat(s: String, n: Int) -> String { repeat(s, n) }
fn good_repeat(n: Int where 0 <= it && it <= 10) -> String { repeat("ab", n) }
fn floats(x: Float) -> Float { x * x }
fn doubled(xs: List[Int]) -> List[Int] { xs.map(x -> x * 2) }
"#;
    save(&d.join("f.strata"), src);
    let (ok, out) = strata(&["--file", "f.strata", "verify", "--cases", "30"], &d, None);
    assert!(!ok, "{out}");
    let line = |name: &str| out.lines().find(|l| l.split_whitespace().nth(1) == Some(name)).unwrap_or("").to_string();
    // (`split` and `repeat` are total: any arguments)
    for name in ["good_chr", "good_ord", "each_ord", "round_trip", "good_repeat", "any_split", "any_repeat", "doubled"] {
        assert!(line(name).starts_with("✓"), "{name} should be proven: {out}");
    }
    for name in ["bad_chr", "bad_ord"] {
        assert!(line(name).starts_with("✗") && line(name).contains("found by the solver"), "{name} should be refuted by the solver: {out}");
    }
    assert!(line("floats").starts_with("~") && out.contains("`*` gives a finite number"), "{out}");
}

#[test]
fn fuzzes_strings_a_refinement_pattern_matches() {
    // inputs made by `std.regex.samples`: Unicode classes and word
    // boundaries too, in a parameter's own refinement (the harness checks it)
    let d = tmp("regex-samples");
    let src = r#"fn shout(s: String where matches(it, "\p{L}{2,4}")) -> String ensures len(it.code_points()) >= 1 { to_upper(s) }
fn edge(s: String where matches(it, "a\b b?|\bx\w+")) -> String ensures len(it.code_points()) >= 0 { to_lower(s) }
fn sku(s: String where matches(it, "[A-Z]{3}-[0-9]{4}")) -> Int ensures it == 99 { len(to_lower(s).code_points()) }
"#;
    save(&d.join("g.strata"), src);
    let (ok, out) = strata(&["--file", "g.strata", "verify", "--cases", "30"], &d, None);
    assert!(!ok && !out.contains("skipped") && !out.contains("internal error"), "{out}");
    assert!(out.lines().any(|l| l.starts_with("~ shout") && l.contains("fuzzed: 30 cases")), "{out}");
    let sku = out.lines().skip_while(|l| !l.starts_with("✗ sku")).nth(1).unwrap_or("");
    assert!(sku.contains("input:   \"") && sku.contains("-"), "refuted, with an input: {out}");
}

#[test]
fn math_example_verifies_as_documented() {
    let d = tmp("math");
    let ex = format!("{}/examples/math.strata", env!("CARGO_MANIFEST_DIR"));
    let (ok, out) = strata(&["--file", &ex, "test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");
    if !has_solver() {
        eprintln!("z3 is not installed: skipping the verification part");
        return;
    }
    let (ok, out) = strata(&["--file", &ex, "verify", "--checks"], &d, None);
    assert!(!ok, "the two bugs fail the run: {out}");
    for name in ["factorial", "gcd", "power", "digit_sum", "ackermann", "triangle", "collatz_steps", "positive_squares", "is_prime", "primes", "first_or_one"] {
        assert!(out.contains(&format!("✓ {name} ")), "{name} should be proven (as docs/verification.md says): {out}");
    }
    assert!(out.contains("~ isqrt") && out.contains("not proven: ensures it * it <= n"), "{out}");
    assert!(out.contains("the pair (`m`, `n`), compared in order") && out.contains("may not terminate: the loop `while x != 1`"), "{out}");
    assert!(out.contains("✗ average") && out.contains("input:   []\n") && out.contains("✗ next_digit") && out.contains("input:   9\n"), "{out}");
}

#[test]
fn smt_knows_what_holds_for_every_element() {
    if !has_solver() {
        eprintln!("z3 is not installed: skipping");
        return;
    }
    let d = tmp("smt-elements");
    let src = text(std::path::Path::new(&format!("{}/tests/data/elements.strata", env!("CARGO_MANIFEST_DIR"))));
    save(&d.join("e.strata"), src);
    let (ok, out) = strata(&["--file", "e.strata", "verify", "--cases", "30"], &d, None);
    assert!(!ok, "{out}");
    for name in ["positives", "doubled", "digits", "lit", "pushed", "joined", "all_from", "first_big", "found", "rest_ok", "empty_is_fine"] {
        assert!(out.contains(&format!("✓ {name} ")), "{name} should be proven: {out}");
    }
    // refuted, by the solver or the fuzzer; `loop_push` was wrongly proven before (a loop kept its list's facts)
    for name in ["bad_positives", "bad_doubled", "bad_digits", "bad_lit", "bad_pushed", "loop_push"] {
        assert!(out.contains(&format!("✗ {name} ")), "{name} should be refuted: {out}");
    }
    assert!(!out.contains("✓ bad_all"), "{out}");
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
  mut i := 0
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
    save(&d.join("s.strata"), src);
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
    save(&d.join("f.strata"), "fn mean(xs: List[Int]) -> Int where it >= 0 { if len(xs) == 0 { 0 } else { abs(xs.fold(0, (a, x) -> a + x)) / len(xs) } }\nfn neg(xs: List[Int]) -> Natural { xs.fold(0, (a, x) -> a + x) }\n");
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
    // an edit that doesn't check in itself is rejected
    let (ok, out) = strata(&["edit"], &d, Some("fn total(lines: List[Cents]) -> Cents { sum(lines) + true }"));
    assert!(!ok && out.contains("rejected"), "{out}");
    // one that breaks a caller isn't committed: it opens a pending change, with a hole for the caller
    let (ok, out) = strata(&["edit"], &d, Some("fn total(lines: List[Cents]) -> Cents { sum(lines) }"));
    assert!(ok && out.contains("it is now a hole, in a pending change") && out.contains("was: total(lines)"), "{out}");
    assert!(strata(&["abort"], &d, None).0);
    let (_, render) = strata(&["render"], &d, None);
    assert!(render.contains("fn total(lines: NonEmpty[Line])"), "aborted: nothing changed");
}

#[test]
fn run_inputs_are_validated_against_refinements() {
    let d = tmp("run-inputs");
    let (ok, out) = strata(&["--file", &example(), "run", "total", r#"[Line { sku: "a", qty: 0, unit_price: 5 }]"#], &d, None);
    assert!(!ok && out.contains("`Line.qty`: 0 does not satisfy `Quantity`"), "{out}");
    let (ok, out) = strata(&["--file", &example(), "run", "total", r#"[Line { sku: "a", qty: 2, unit_price: 5 }]"#], &d, None);
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
    save(&d.join("e.strata"), "fn f(x: Int) -> Float { x * 1.5 }\nfn g(b: Bool) -> Int { len(b) }\n");
    let (ok, out) = strata(&["--file", "e.strata", "check"], &d, None);
    assert!(!ok && out.contains("no version of `len` accepts (Bool)") && out.contains("expected `Int`, found `Float`"), "{out}");
}

#[test]
fn every_example_round_trips_through_render() {
    let d = tmp("roundtrip");
    for ex in ["bank.strata", "orders.strata", "builtins.strata", "traits.strata"] {
        let path = format!("{}/examples/{ex}", env!("CARGO_MANIFEST_DIR"));
        let (_, r1) = strata(&["--file", &path, "render"], &d, None);
        save(&d.join("r.strata"), &r1);
        let (_, r2) = strata(&["--file", "r.strata", "render"], &d, None);
        assert_eq!(r1, r2, "{ex} does not round-trip");
        let (ok, out) = strata(&["--file", "r.strata", "test"], &d, None);
        assert!(ok, "{ex} tests fail after round-trip: {out}");
    }
    // a package: one file per module
    let path = format!("{}/examples/tasks", env!("CARGO_MANIFEST_DIR"));
    let (ok, out) = strata(&["--file", &path, "render", "--split", "t1"], &d, None);
    assert!(ok, "{out}");
    // the text read back in, module by module, prints the same
    std::fs::create_dir_all(d.join("t2")).unwrap();
    std::fs::copy(format!("{path}/strata.toml"), d.join("t2/strata.toml")).unwrap();
    for m in ["core", "memory", "print", "sim"] {
        save(&d.join(format!("t2/{m}.strata")), std::fs::read_to_string(d.join(format!("t1/{m}.st"))).unwrap());
    }
    let (ok, out) = strata(&["--file", "t2", "render", "--split", "t3"], &d, None);
    assert!(ok, "{out}");
    for m in ["core", "memory", "print", "sim"] {
        let f = format!("{m}.st");
        assert_eq!(std::fs::read_to_string(d.join("t1").join(&f)).unwrap(), std::fs::read_to_string(d.join("t3").join(&f)).unwrap(), "tasks/{f} does not round-trip");
    }
    let (ok, out) = strata(&["--file", "t2", "test"], &d, None);
    assert!(ok, "tasks tests fail after round-trip: {out}");
}

#[test]
fn inline_refinements_in_signatures() {
    let d = tmp("inline");
    save(&d.join("i.strata"),
        "fn pick(level: Int where 5 <= it && it <= 13) -> Positive { level - 4 }\n\
         fn halve_up(n: Natural) -> Int where it * 2 <= n { (n + 1) / 2 }\n");
    let (ok, out) = strata(&["--file", "i.strata", "verify"], &d, None);
    assert!(!ok && passed(&out, "pick") && out.contains("does not satisfy `Int where it * 2 <= n`"), "{out}");
    save(&d.join("c.strata"), "fn pick(level: Int where 5 <= it && it <= 13) -> Int { level }\nfn caller() -> Int { pick(20) }\n");
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
    save(&d.join("e.strata"),
        "type Name = String\n\
         distinct type UserId = String where matches(it, \"u_[0-9]+\")\n\
         distinct type OrderId = String where matches(it, \"o_[0-9]+\")\n\
         trait Priced { fn price(self) -> Int }\n\
         fn mixed(o: OrderId) -> UserId { o }\n\
         fn lit() -> UserId { UserId(\"alice\") }\n\
         fn nobound[T](xs: List[T]) -> Int { sum(xs.map(price)) }\n\
         fn notimpl() -> Int { true.price() }\n");
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
        assert_eq!(text(std::path::Path::new(&format!("{ex}/{m}.strata"))), std::fs::read_to_string(d.join(format!("split/{m}.st"))).unwrap(), "{m}");
    }
    // `render` prints one module; with several it asks which
    let (ok, out) = strata(&["--file", &ex, "render", "core"], &d, None);
    assert!(ok && out == text(std::path::Path::new(&format!("{ex}/core.strata"))), "{out}");
    let (ok, out) = strata(&["--file", &ex, "render"], &d, None);
    assert!(!ok && out.contains("4 modules, one file each"), "{out}");
    // `module` blocks are gone: a file is a module
    let (ok, _) = strata(&["--file", "blocks.strata", "edit"], &d, Some("module a {\n  fn f() -> Int {\n    1\n  }\n}\n"));
    assert!(!ok, "`module a {{ }}` must not parse");
    // the contract shows public definitions only
    let (ok, out) = strata(&["--file", &ex, "interface", "core"], &d, None);
    assert!(ok && out.contains("pub fn run_worker") && !out.contains("same_ids"), "{out}");
    // private definitions are not part of the contract
    std::fs::create_dir_all(d.join("p")).unwrap();
    save_module(&d.join("p"), "a", "pub fn open() -> Int { secret() }\nfn secret() -> Int { 1 }\n");
    save_module(&d.join("p"), "b", "use a.*\nfn f() -> Int { open() + secret() }\nfn g() -> Int { a.secret() }\n");
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
    let (ok, out) = strata(&["edit", "core"], &pkg, Some("fn same_ids(claimed: List[TaskId], reported: List[TaskId]) -> Bool { len(claimed) == len(reported) && all(claimed, c -> count(reported, r -> r == c) == 1) && all(reported, r -> contains(claimed, r)) }"));
    assert!(ok && out.contains("core.same_ids") && out.contains("implementation only"), "{out}");
    assert!(text(&pkg.join("core.strata")).contains("all(reported, r -> contains(claimed, r))"), "the edit is in the file");
    let (ok, out) = strata(&["test"], &pkg, None);
    assert!(ok && out.contains("0 failed"), "{out}");
}

#[test]
fn procedural_mutation_is_explicit() {
    let d = tmp("mutation");
    save(&d.join("m.strata"),
        "type Line = { qty: Int where 1 <= it && it <= 99 }\n\
         fn restock(mut line: Line, extra: Int) { line.qty = line.qty + extra }\n\
         fn sum(xs: List[Int]) -> Int {\n  mut s := 0\n  for x in xs { s = s + x }\n  s\n}\n\
         test \"ok\" {\n  mut l := Line { qty: 2 }\n  restock(mut l, 3)\n  assert l.qty == 5\n  assert sum([1, 2, 3]) == 6\n}\n\
         test \"refined\" {\n  mut l := Line { qty: 98 }\n  restock(mut l, 5)\n}\n");
    let (ok, out) = strata(&["--file", "m.strata", "test"], &d, None);
    assert!(!ok && out.contains("✓ ok") && out.contains("103 does not satisfy"), "{out}");
    save(&d.join("e.strata"), "fn f() -> Int {\n  x := 1\n  x = 2\n  x\n}\nfn g() -> Int {\n  mut y := 0\n  add := z -> { y = y + z; y }\n  add(1)\n}\n");
    let (ok, out) = strata(&["--file", "e.strata", "check"], &d, None);
    assert!(!ok && out.contains("`x`: it is not mutable") && out.contains("a closure cannot change `y`"), "{out}");

    // a call that can change an argument says so: `mut` at the call site
    save(&d.join("c.strata"),
        "type Line = { qty: Int }\n\
         trait Counter { fn tick(mut self) -> ()  fn peek(self) -> Int }\n\
         impl Counter for Line { fn tick(mut self) { self.qty = self.qty + 1 }  fn peek(self) -> Int { self.qty } }\n\
         fn restock(mut line: Line, extra: Int) { line.qty = line.qty + extra }\n\
         fn qty(line: Line) -> Int { line.qty }\n\
         fn unmarked() -> Int {\n  mut l := Line { qty: 1 }\n  restock(l, 2)\n  l.qty\n}\n\
         fn needless() -> Int {\n  mut l := Line { qty: 1 }\n  qty(mut l)\n}\n\
         fn builtin() -> Int {\n  mut xs := [1]\n  len(mut xs)\n}\n\
         fn receiver() -> Int {\n  mut l := Line { qty: 1 }\n  mut l.tick()\n  restock(mut l, 1)\n  l.qty\n}\n\
         fn unmarked_receiver() {\n  mut l := Line { qty: 1 }\n  l.tick()\n}\n\
         fn needless_receiver() -> Int {\n  mut l := Line { qty: 1 }\n  mut l.peek()\n}\n\
         fn stray() -> Int {\n  mut n := 1\n  m := mut n\n  m\n}\n\
         fn first(ls: List[Line]) -> Line { Line { qty: 1 } }\n",
    );
    let (ok, out) = strata(&["--file", "c.strata", "check"], &d, None);
    assert!(!ok, "{out}");
    assert!(out.contains("in unmarked: `restock` can change its `mut line` parameter: write `mut l` to show that `l` may change"), "{out}");
    assert!(out.contains("in needless: `mut` on an argument of `qty`, but its parameter `line` is not `mut`"), "{out}");
    assert!(out.contains("in builtin:"), "{out}");
    assert!(!out.contains("in receiver"), "{out}");
    // a method that changes its receiver is marked too: `mut l.tick()`
    assert!(out.contains("in unmarked_receiver: `tick` changes its receiver (`mut self`): write `mut l.tick(…)`"), "{out}");
    assert!(out.contains("in needless_receiver: `mut l.peek(…)`: `peek` doesn't change its receiver"), "{out}");
    assert!(out.contains("in stray: `mut n` marks an argument"), "{out}");
    // results are not places: name them first
    let (ok, out) = strata(&["--file", "r.strata", "edit"], &d, Some("fn f(xs: List[Int]) {\n  mut first(xs).tick()\n}\n"));
    assert!(!ok && out.contains("only a variable or a field can be `mut`"), "{out}");
    let (ok, out) = strata(&["--file", "c.strata", "render"], &d, None);
    assert!(ok && out.contains("restock(mut l, 1)"), "{out}");
}

#[test]
fn loops_chains_and_80_columns() {
    let d = tmp("fmt");
    save(&d.join("l.strata"),
        "fn steps(start: Positive) -> Natural {\n  mut n := start\n  mut k := 0\n  loop {\n    if n == 1 { break }\n    n = if n % 2 == 0 { n / 2 } else { 3 * n + 1 }\n    k = k + 1\n  }\n  k\n}\n\
         fn top(xs: List[Int]) -> Int {\n  xs\n    .filter(x -> x > 0)\n    .sort()\n    .reverse()\n    .take(3)\n    .sum()\n}\n\
         test \"t\" {\n  assert steps(6) == 8\n  assert top([5, -1, 9, 2, 7]) == 21\n}\n");
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
  n := 1 // one


  m := n
  match s {
    // still going
    Pending => "pending",
    Done => "done", // all good
  }
}
"#;
    save(&d.join("c.strata"), src);
    let (ok, r1) = strata(&["--file", "c.strata", "render"], &d, None);
    assert!(ok, "{r1}");
    for c in ["// file header\n//\n// second paragraph\n\n// section banner\n/// A status.", "  // waiting\n  | Pending", "  // finished\n  | Done",
        "// the y axis points down", "  // area, never negative\n  /// The area.", "  // checked by verify\n  /// Areas are never negative.\n  law nonneg", "  // a point has no area\n  /// Always zero.",
        "  // the common case first\n  // one\n  n := 1\n\n  m := n\n  match", "    // still going\n    Pending", "    // all good\n    Done"] {
        assert!(r1.contains(c), "missing {c:?} in\n{r1}");
    }
    save(&d.join("r1.strata"), &r1);
    let (_, r2) = strata(&["--file", "r1.strata", "render"], &d, None);
    assert_eq!(r1, r2);
    let (ok, out) = strata(&["--file", "c.strata", "check"], &d, None);
    assert!(ok, "{out}");

    // a comment the tree has no place for is an error, not silently dropped
    let (ok, out) = strata(&["--file", "bad.strata", "edit"], &d, Some("fn f(a: Int, // why\n  b: Int) -> Int { a }\n"));
    assert!(!ok && out.contains("1:14: this comment would be lost"), "{out}");

    // editing a comment changes the definition, but not its interface
    let hash = |file: &str| {
        let (_, out) = strata(&["--file", file, "ast", "Point"], &d, None);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        (v["hash"].as_str().unwrap().to_string(), v["interface_hash"].as_str().unwrap().to_string())
    };
    save(&d.join("c2.strata"), src.replace("points down", "points up"));
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
  n := size(xs)
  tally := x -> x + n
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
    assert!(strata(&["init"], &d, None).0);
    save(&d.join("a.strata"), a);
    save(&d.join("b.strata"), b);

    // a name a local or another definition has can't capture a reference: each
    // is stored in full, and where the short name is taken, shown in full
    let e = tmp("rename-capture");
    std::fs::copy(d.join("strata.toml"), e.join("strata.toml")).unwrap();
    for f in ["a.strata", "b.strata"] {
        std::fs::copy(d.join(f), e.join(f)).unwrap();
    }
    let (ok, out) = strata(&["rename", "a.size", "count", "--no-commit"], &e, None);
    assert!(ok, "{out}");
    let b2 = text(&e.join("b.strata"));
    assert!(b2.contains("a.count(b.items) + a.count([]) + count(1)"), "{b2}");
    let (ok, out) = strata(&["rename", "a.count", "tally", "--no-commit"], &e, None);
    assert!(ok && text(&e.join("a.strata")).contains("a.tally(xs)"), "{out}");
    let (ok, out) = strata(&["test"], &e, None);
    assert!(ok, "{out}");
    let (ok, out) = strata(&["rename", "Box", "crate"], &d, None);
    assert!(!ok && out.contains("uppercase"), "{out}");

    let (ok, out) = strata(&["rename", "size", "length"], &d, None);
    assert!(ok && out.contains("renamed a.size → a.length") && out.contains("a.twice") && out.contains("b.total"), "{out}");
    assert!(out.contains("docs or comments still mention `size` in: a.length"), "{out}");
    let (ok, out) = strata(&["rename", "a.Box", "Crate"], &d, None);
    assert!(ok, "{out}");
    let r = strata(&["render", "a"], &d, None).1 + &strata(&["render", "b"], &d, None).1;
    for s in ["pub fn length(xs: List[Int])", "n := length(xs)", "tally(length(xs))", "use a.{length, Crate}", "length(b.items) + length([])", "fn total(b: Crate)", "Crate { items: [1, 2] }"] {
        assert!(r.contains(s), "missing {s:?} in\n{r}");
    }
    let (ok, out) = strata(&["test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");
    let (_, log) = strata(&["log", "--json"], &d, None);
    assert!(log.contains(r#""from":"a.size""#) && log.contains(r#""to":"a.length""#), "{log}");
    // the files are the source; the log is next to them, to be committed
    assert!(text(&d.join("b.strata")).contains("fn total(b: Crate)"));
    assert_eq!(std::fs::read_to_string(d.join("strata.log")).unwrap().lines().count(), 2);
    // the log replays backwards from the files: the first step starts from what was written by hand
    let (ok, out) = strata(&["diff", "--from", "0", "--step", "0"], &d, None);
    assert!(ok && out.contains("↪ a.length  (renamed from `size`)") && out.contains("3 references updated") && !out.contains("a.Crate  (renamed"), "{out}");
}

/// Run the `test` blocks of `src` and check that it round-trips through `render`.
fn run_tests(name: &str, src: &str, passed: &str) {
    let d = tmp(&format!("lang-{name}"));
    save(&d.join("a.strata"), src);
    let (ok, out) = strata(&["--file", "a.strata", "test"], &d, None);
    assert!(ok && out.contains(passed), "{out}");
    let (_, r1) = strata(&["--file", "a.strata", "render"], &d, None);
    save(&d.join("r1.strata"), &r1);
    let (_, r2) = strata(&["--file", "r1.strata", "render"], &d, None);
    assert_eq!(r1, r2);
}

/// `check` on `src` fails with a message containing `msg`.
/// `src` is refused: it doesn't parse (the edit storing it fails), or it
/// doesn't check.
fn check_fails(name: &str, src: &str, msg: &str) {
    let d = tmp(&format!("lang-{name}"));
    let (ok, out) = strata(&["--file", "a.strata", "edit"], &d, Some(src));
    let (ok, out) = if ok { strata(&["--file", "a.strata", "check"], &d, None) } else { (ok, out) };
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
  assert len("héllo".code_points()) == 5 && len("héllo".bytes()) == 6
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
  k := () -> 7
  assert k() == 7
  f := x -> {
    y := x + 1
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
  x := parse(a)!
  Ok(x + parse(b)!)
}

fn first_even(xs: List[Int]) -> Option[Int] {
  x := xs.find(x -> x % 2 == 0)?
  Some(x * 10)
}

fn or_zero(s: String) -> Int { parse(s) except { 0 } }

fn describe(s: String) -> String {
  n := parse(s) except e {
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
  f := x -> {
    if x > 1 { return "big" }
    "small"
  }
  assert describe("7") == "the number 7"
  assert describe("q") == "not a number: q"
  assert describe("") == "empty"
  assert f(5) == "big" && f(0) == "small"
  assert [1, 2, 3].find(x -> x > 5) or { 9 } == 9
}
"#,
        "1 passed",
    );
    let pre = "fn p(s: String) -> Result[Int, String] { Err(s) }\n";
    check_fails("bang", &format!("{pre}fn f() -> Int {{ p(\"a\")! }}"), "`!` returns the error early, so the result type must be `Result[");
    check_fails("bangerr", &format!("{pre}fn f() -> Result[Int, Int] {{ Ok(p(\"a\")!) }}"), "`!` returns the error early");
    check_fails("qonresult", &format!("{pre}fn f() -> Option[Int] {{ Some(p(\"a\")?) }}"), "`?` needs an Option");
    check_fails("exceptoption", "fn f() -> Int { [1].find(x -> x > 0) except e { 0 } }", "an Option is handled with `or");
    check_fails("exceptint", "fn f() -> Int { 1 except { 0 } }", "`except` / `!!` handle a Result, found `Int`");
    check_fails("orint", "fn f() -> Int { 1 ?? 0 }", "`or` / `??` handle an Option, found `Int`");
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
  n := 3
  assert f"{n} + 1 = {n + 1}" == "3 + 1 = 4"
  assert f"{{literal}} {n}" == "{literal} 3"
  assert f"quote \" and {join(["a", "b"], "-")}" == "quote \" and a-b"
  assert f"{Red} {[1, 2]} {Some(1)}" == "Red [1, 2] Some(1)"
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
  s := "héllo wörld"
  assert len(s.code_points()) == 11 && len(s.graphemes()) == 11 && len(s.bytes()) == 13 && s.byte_len() == 13
  assert s.chars().take(2) == ["h", "é"]
  assert s.graphemes().get(1) == Some("é") && s.graphemes().get(99) == None
  assert s.byte_slice(7, 13) == "wörld" && s.byte_slice(-3, 3) == "hé" && s.byte_slice(8, 3) == "" && s.byte_slice(0, 2) == "h"
  assert [1, 2, 3, 4].slice(1, 3) == [2, 3]
  assert s.index_of("wö") == Some(7) && s.index_of("z") == None
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
    save(&d.join("u.strata"), UNIONS);
    save(&d.join("t.strata"), TRAITS_PATHS);
    for f in ["u.strata", "t.strata"] {
        let (ok, out) = strata(&["--file", f, "test"], &d, None);
        assert!(ok && out.contains("1 passed"), "{out}");
        let (ok, r1) = strata(&["--file", f, "render"], &d, None);
        assert!(ok, "{r1}");
        save(&d.join("r.strata"), &r1);
        let (_, r2) = strata(&["--file", "r.strata", "render"], &d, None);
        assert_eq!(r1, r2);
    }
    // an argument of a union type is a value of one of its members
    let (ok, out) = strata(&["--file", "u.strata", "run", "area", "Circle { radius: 2 }"], &d, None);
    assert!(ok && out.trim() == "12", "{out}");
    let (ok, out) = strata(&["--file", "u.strata", "run", "max_of", "NonEmptyErr"], &d, None);
    assert!(!ok && out.contains("expected `TooBig`, found `ShapeError`"), "{out}");

    save(&d.join("bad.strata"), BAD);
    let (ok, out) = strata(&["--file", "bad.strata", "check"], &d, None);
    assert!(!ok);
    for e in [
        "non-exhaustive match on `Int`: `where 3 <= it && it <= 5` is not covered",
        "non-exhaustive match on `Int | String`: `String` is not covered",
        "unreachable match arm `Positive`",
        "unreachable match arm `7`",
        "pattern `String`: expected `Circle | Square`, found `String`",
        "non-exhaustive match on `String`: `where len(it.code_points()) == 0` is not covered",
        "not every `Circle | Square` does",
    ] {
        assert!(out.contains(e), "missing {e:?} in\n{out}");
    }
    assert!(!out.contains("unreachable match arm `String`"), "{out}");

    // a variant is a type, so its name is unique in its module; two modules
    // may each have a `Done` (told apart as `a.Done`, `b.Done`)
    std::fs::create_dir_all(d.join("clash")).unwrap();
    save_module(&d.join("clash"), "a", "pub type Status = | Done | Open\n");
    save_module(&d.join("clash"), "b", "pub type Outcome = | Done | Failed\n");
    let (ok, out) = strata(&["--file", "clash", "check"], &d, None);
    assert!(ok, "{out}");
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
    String => f"text {len(v.code_points())}",
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
  mut x := n
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
  x := if true { Square { side: 1 } } else { Circle { radius: 1 } }
  1
}
"#;

#[test]
fn or_patterns_refined_scrutinees_lengths_floats_and_variance() {
    let d = tmp("patterns2");
    for (f, src) in [("or.strata", OR_PATTERNS), ("dom.strata", DOMAINS)] {
        save(&d.join(f), src);
        let (ok, out) = strata(&["--file", f, "test"], &d, None);
        assert!(ok && out.contains("1 passed"), "{out}");
        let (ok, r1) = strata(&["--file", f, "render"], &d, None);
        assert!(ok, "{r1}");
        save(&d.join("r.strata"), &r1);
        let (_, r2) = strata(&["--file", "r.strata", "render"], &d, None);
        assert_eq!(r1, r2);
    }
    let check = |name: &str, src: &str| {
        save(&d.join(name), src);
        strata(&["--file", name, "check"], &d, None)
    };
    let (ok, out) = check("bad.strata", DOMAINS_BAD);
    assert!(!ok);
    for e in [
        "non-exhaustive match on `Positive`: `2` is not covered",
        "unreachable match arm `where it <= 0`: no `Positive` matches it",
        "non-exhaustive match on `Float`: `where it == 0.0` is not covered",
        "non-exhaustive match on `String`: `where len(it.code_points()) <= 3` is not covered",
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
    save(&d.join("mut_run.strata"), "fn a(mut n: Positive) -> Int {\n  n = n - 10\n  n\n}\n");
    let (ok, out) = strata(&["--file", "mut_run.strata", "run", "a", "3"], &d, None);
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
  match s { where len(it.code_points()) == 0 => "empty", where len(it.code_points()) > 0 => "some" }
}
fn ne(s: NonEmptyString) -> String {
  match s { where len(it.code_points()) == 1 => "char", where len(it.code_points()) >= 2 => "word" }
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
  match s { where len(it.code_points()) > 3 => "long" }
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
fn grow(mut shapes: List[Shape]) { shapes = shapes + [Square { side: 1 }] }
fn mixed(big: Bool) -> Int {
  s := if big { Square { side: 2 } } else { Circle { radius: 1 } }
  xs := [Square { side: 1 }, Circle { radius: 1 }]
  total(xs)
}
fn ok_list() -> Int {
  cs := [Circle { radius: 1 }]
  total(cs)
}
fn bad_mut() -> Int {
  mut cs := [Circle { radius: 1 }]
  grow(mut cs)
  len(cs)
}
fn inner() -> Result[Int, TooBig] { Err(TooBig) }
fn outer() -> Result[Int, Err1] {
  x := inner()!
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
  mut x: Positive = 3
  x = -1
  x
}
fn set_neg(mut k: Int) { k = -3 }
fn wb() -> Int {
  mut p: Positive = 4
  set_neg(mut p)
  p
}
fn bump(mut k: Positive) { k = k + 1 }
fn wb_ok() -> Int {
  mut p: Natural = 1
  bump(mut p)
  p
}
"#;

/// Run `strata merge-file` on three module files, given as text: (exit ok,
/// merged text, output). A conflict leaves the file as text.
fn merge3(name: &str, base: &str, ours: &str, theirs: &str) -> (bool, String, String) {
    let d = tmp(name);
    for (f, t) in [("o.strata", base), ("a.strata", ours), ("b.strata", theirs)] {
        save(&d.join(f), t);
    }
    let (ok, out) = strata(&["merge-file", "o.strata", "a.strata", "b.strata", "a.strata"], &d, None);
    let bytes = std::fs::read(d.join("a.strata")).unwrap();
    let merged = if bytes.starts_with(b"STRATA") { text(&d.join("a.strata")) } else { String::from_utf8(bytes).unwrap() };
    (ok, merged, out)
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
    let base = "fn f(x: Int) -> Int {\n  a := x + 1\n  b := a * 2\n  c := b - 3\n  c\n}\n";
    let ours = "fn f(x: Int) -> Int {\n  a := x + 100\n  b := a * 2\n  c := b - 3\n  c\n}\n";
    let theirs = "fn f(x: Int) -> Int {\n  a := x + 1\n  b := a * 2\n  c := b - 300\n  c\n}\n";
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

    // sides that aren't module files are left alone: ours stays
    let d = tmp("merge-text");
    for (f, t) in [("o.strata", "fn (\n"), ("a.strata", "fn (\nours\n"), ("b.strata", "fn (\n")] {
        std::fs::write(d.join(f), t).unwrap();
    }
    let (ok, out) = strata(&["merge-file", "o.strata", "a.strata", "b.strata", "a.strata"], &d, None);
    assert!(!ok && out.contains("can't merge") && std::fs::read_to_string(d.join("a.strata")).unwrap() == "fn (\nours\n", "{out}");
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
    // a new repository has its own identity, so commits strata makes don't
    // depend on the machine's git configuration
    if args.first() == Some(&"init") {
        git(dir, &["config", "user.name", "t"]);
        git(dir, &["config", "user.email", "t@example.com"]);
    }
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
    // main is stored before orders exists: its header says `run` uses nothing
    save(&d.join("shop/main.strata"), "use orders.total\n\nfn run() -> Int {\n  total([1, 2])\n}\n");
    save(&d.join("shop/orders.strata"), "pub fn total(xs: List[Int]) -> Int { sum(xs) }\n");
    // a package nested in another is a separate program: its `use` doesn't see shop's modules
    std::fs::write(d.join("shop/tools/strata.toml"), "name = \"tools\"\n").unwrap();
    save(&d.join("shop/tools/main.strata"), "fn one() -> Int {\n  1\n}\n");
    // a file outside every package is a script
    save(&d.join("loose.strata"), "fn x() -> Int {\n  1\n}\n");
    let (ok, out) = strata_git(&["ci"], &d);
    assert!(!ok && out.contains("shop/main.strata: not in canonical form"), "{out}");
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
    save(&d.join("app.strata"), base);
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "base"]);
    git(&d, &["checkout", "-q", "-b", "feature"]);
    save(&d.join("app.strata"), "fn g(x: Int) -> Int {\n  x * 2\n}\n");
    git(&d, &["commit", "-q", "-am", "feature: drop f"]);
    git(&d, &["checkout", "-q", "main"]);
    save(&d.join("app.strata"), base.replace("g(x: Int) -> Int {\n  x * 2", "g(y: Int) -> Int {\n  y * 20"));
    git(&d, &["commit", "-q", "-am", "main: g"]);
    git(&d, &["checkout", "-q", "feature"]);

    // plain git conflicts: f's deletion touches the line before g's signature
    let (ok, out) = strata_git(&["sync", "main"], &d);
    assert!(ok && out.contains("merged main") && out.contains("✓ app.strata"), "{out}");
    let merged = text(&d.join("app.strata"));
    assert_eq!(merged, "fn g(y: Int) -> Int {\n  y * 20\n}\n");
    assert!(git(&d, &["log", "-1", "--format=%p"]).split_whitespace().count() == 2, "a merge commit");
    let (ok, out) = strata_git(&["sync", "main"], &d);
    assert!(ok && out.contains("already up to date"), "{out}");

    // a merge that doesn't check is left staged, not committed
    git(&d, &["checkout", "-q", "main"]);
    save(&d.join("app.strata"), merged.replace("fn g(y: Int)", "fn g(x: Int, y: Int)").replace("y * 20", "x * y"));
    git(&d, &["commit", "-q", "-am", "main: g takes two"]);
    git(&d, &["checkout", "-q", "feature"]);
    save(&d.join("app.strata"), merged.clone() + "\nfn h() -> Int {\n  g(1)\n}\n");
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
    assert!(strata(&["init"], &d, None).0);
    save(&d.join("a.strata"), "pub fn area(w: Int, h: Int) -> Int {\n  w * h\n}\n");
    save(&d.join("b.strata"), "use a.area\n\nfn square(s: Int) -> Int {\n  area(s, s)\n}\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "base"]);

    // one commit: a rename and an edit through strata, then a change by hand
    assert!(strata(&["rename", "area", "rect_area", "--no-commit"], &d, None).0);
    let (ok, out) = strata(&["edit", "--no-commit", "a", "-m", "areas are never negative"], &d, Some("pub fn rect_area(w: Int, h: Int) -> Int requires w >= 0 && h >= 0 {\n  w * h\n}\n"));
    assert!(ok, "{out}");
    let b = text(&d.join("b.strata"));
    assert!(b.contains("rect_area(s, s)"), "the rename rewrote the other file: {b}");
    save(&d.join("b.strata"), b.replace("rect_area(s, s)\n}", "rect_area(s, s)\n}\n\nfn cube(s: Int) -> Int {\n  square(s) * s\n}"));
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
    assert!(strata(&["init"], &d, None).0);
    save(&d.join("a.strata"), "pub fn area(w: Int, h: Int) -> Int {\n  w * h\n}\n");
    save(&d.join("b.strata"), "use a.area\n\nfn square(s: Int) -> Int {\n  area(s, s)\n}\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "base"]);

    // feature renames `area`; main adds a new file that calls it by its old name
    git(&d, &["checkout", "-q", "-b", "feature"]);
    assert!(strata(&["rename", "area", "rect_area"], &d, None).0);
    git(&d, &["checkout", "-q", "main"]);
    save(&d.join("c.strata"), "use a.area\n\nfn cube(s: Int) -> Int {\n  area(s, s) * s\n}\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "cubes"]);
    git(&d, &["checkout", "-q", "feature"]);

    let (ok, out) = strata_git(&["sync", "main"], &d);
    assert!(ok && out.contains("merged main"), "{out}");
    assert!(out.contains("merge: theirs code follows the rename a.area → a.rect_area (c.use a, c.cube)"), "{out}");
    let c = text(&d.join("c.strata"));
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
    git(&d, &["checkout", "-q", "feature"]);
    save(&d.join("b.strata"), text(&d.join("b.strata")) + "\nfn twice(s: Int) -> Int {\n  rect_area(s, s) * 2\n}\n");
    git(&d, &["commit", "-q", "-am", "twice"]);
    let (ok, out) = strata_git(&["sync", "main"], &d);
    assert!(ok && out.contains("merged main"), "{out}");
    let b = text(&d.join("b.strata"));
    assert!(b.contains("surface(s, s) * 2") && !b.contains("rect_area"), "{b}");

    // a rename the other side's code can't follow (a local would capture the new name) is left alone
    git(&d, &["checkout", "-q", "main"]);
    git(&d, &["merge", "-q", "feature"]);
    assert!(strata(&["rename", "surface", "extent"], &d, None).0);
    git(&d, &["checkout", "-q", "feature"]);
    save(&d.join("b.strata"), b.clone() + "\nfn padded(s: Int) -> Int {\n  extent := 2\n  surface(s, s) + extent\n}\n");
    git(&d, &["commit", "-q", "-am", "padded"]);
    let head = git(&d, &["rev-parse", "HEAD"]);
    let (ok, out) = strata_git(&["sync", "main"], &d);
    assert!(!ok && out.contains("doesn't check") && out.contains("b.padded"), "{out}");
    assert!(text(&d.join("b.strata")).contains("surface(s, s) + extent"), "not captured");
    assert_eq!(git(&d, &["rev-parse", "HEAD"]), head, "nothing committed");
}

/// Run strata and give (exit code, stdout, stderr).
fn strata_code(args: &[&str], dir: &std::path::Path, stdin: &str) -> (i32, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_strata")).args(args).current_dir(dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string())
}

#[test]
fn std_io_programs_run_with_capabilities_and_exit_codes() {
    let d = tmp("stdio");
    let ex = |p: &str| format!("{}/examples/{p}", env!("CARGO_MANIFEST_DIR"));

    // main gets the console and the command line; `()` exits 0
    let (code, out, _) = strata_code(&["run", &ex("hello"), "--", "Ada", "Alan"], &d, "");
    assert_eq!((code, out.as_str()), (0, "hello, Ada and Alan!\n"));

    // files, standard input, and an Err from main: written to stderr, exit code 1
    std::fs::write(d.join("a.txt"), "one two\nthree\n").unwrap();
    let (code, out, _) = strata_code(&["run", &ex("wc"), "--", "a.txt"], &d, "");
    assert_eq!((code, out.as_str()), (0, "2\t3\t14\ta.txt\n"));
    let (code, out, _) = strata_code(&["run", &ex("wc")], &d, "a b\nc\n");
    assert_eq!((code, out.as_str()), (0, "2\t3\t6\t-\n"));
    let (code, _, err) = strata_code(&["run", &ex("wc"), "--", "missing.txt"], &d, "");
    assert!(code == 1 && err.contains("NotFound") && err.contains("missing.txt"), "{code} {err}");

    // the same program's tests use stand-ins
    let (ok, out) = strata(&["--file", &ex("wc"), "test"], &d, None);
    assert!(ok && out.contains("4 passed"), "{out}");

    // an exit code, writing files and environment variables
    save(&d.join("prog.strata"),
        r#"use std.io.Console
use std.fs.{Disk, FsError}
use std.process.{Process, ExitCode}

fn main(mut console: Console, mut files: Disk, env: Process) -> Result[ExitCode, FsError] {
  mut files.write("out/x.txt", "a")!
  mut files.append("out/x.txt", "b")!
  mut console.eprint(files.read("out/x.txt")!)
  mut console.write(env.var("STRATA_TEST_VAR") or { "unset" })
  Ok(len(files.list("out")!) + 6)
}
"#);
    std::fs::create_dir_all(d.join("out")).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_strata")).args(["run", "prog.strata"]).current_dir(&d).env("STRATA_TEST_VAR", "set!").output().unwrap();
    assert_eq!(o.status.code(), Some(7), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(String::from_utf8_lossy(&o.stdout), "set!");
    assert_eq!(String::from_utf8_lossy(&o.stderr), "ab\n");

    // the host is the standard library's alone
    save(&d.join("sneaky.strata"), "fn main() {\n  host_write(1, \"hi\")\n}\n");
    let (code, out, err) = strata_code(&["run", "sneaky.strata"], &d, "");
    assert!(code != 0 && out.is_empty() && err.contains("internal to the standard library"), "{out}{err}");

    // `main` may only ask for capabilities the runtime has
    save(&d.join("odd.strata"), "fn main(n: Int) {\n  ()\n}\n");
    let (code, _, err) = strata_code(&["run", "odd.strata"], &d, "");
    assert!(code != 0 && err.contains("can ask for `Console`"), "{err}");

    // verify never reaches the real system: capabilities are mocks there
    save(&d.join("greet.strata"), "use std.io.Console\n\nfn greet(mut c: Console, n: Int) -> () {\n  mut c.print(f\"MARKER {n}\")\n}\n");
    let (_, out, err) = strata_code(&["--file", "greet.strata", "verify", "--cases", "20"], &d, "");
    assert!(!out.contains("MARKER") && !err.contains("MARKER"), "{out}{err}");
    let (ok, out) = strata(&["--file", &format!("{}/std", env!("CARGO_MANIFEST_DIR")), "check"], &d, None);
    assert!(ok, "{out}");
    // std's real terminal, disk and process are assumed, never run by verify
    // and `proven` accepts them, listing them
    let (ok, out) = strata(&["--file", &format!("{}/std", env!("CARGO_MANIFEST_DIR")), "verify", "--cases", "10", "--require", "proven"], &d, None);
    assert!(ok && !out.contains("needs the real system"), "{out}");
    assert!(out.lines().any(|l| l.starts_with("≈ std.io.Terminal.print") && l.contains("assumed: `Terminal` is a capability")), "{out}");
    assert!(out.contains("policy: `proven` accepts") && out.contains("`std.fs.Disk.remove`") && !out.contains("is not proven"), "{out}");
    let (_, out) = strata(&["--file", &format!("{}/std", env!("CARGO_MANIFEST_DIR")), "verify", "--json"], &d, None);
    assert!(out.contains(r#""status": "assumed""#), "{out}");
}

/// A record literal heading a loop, `if`, `match` or contract is rendered
/// in parentheses, as it must be written: the text reads back the same.
#[test]
fn record_literals_in_conditions_round_trip() {
    let d = tmp("brace-heads");
    let src = "type W = { ok: Bool }\n\nfn f(n: Int) -> Int\n  requires (W { ok: true }).ok\n{\n  while (W { ok: false }).ok {\n    print(1)\n  }\n  for x in (Dict { 1: 2 }) {\n    print(x)\n  }\n  if (W { ok: n > 0 }) == (W { ok: true }) {\n    print(2)\n  }\n  match (W { ok: true }) {\n    W { ok } => if ok { 1 } else { 2 },\n  }\n}\n";
    let (ok, out) = strata(&["--file", "a.strata", "edit"], &d, Some(src));
    assert!(ok, "{out}");
    let (ok, text) = strata(&["--file", "a.strata", "render"], &d, None);
    assert!(ok, "{text}");
    for line in ["requires (W { ok: true }.ok)", "while (W { ok: false }.ok) {", "for x in (Dict { 1: 2 }) {", "if (W { ok: n > 0 } == W { ok: true }) {", "match (W { ok: true }) {"] {
        assert!(text.contains(line), "{line}\n{text}");
    }
    let (ok, out) = strata(&["--file", "b.strata", "edit"], &d, Some(&text));
    assert!(ok, "{out}");
    let (_, again) = strata(&["--file", "b.strata", "render"], &d, None);
    assert_eq!(again, text);
}

#[test]
fn string_escapes_round_trip() {
    let d = tmp("escapes");
    save(&d.join("e.strata"), "fn s() -> String {\n  \"a\tb\\\\n\"\n}\n");
    let (_, r1) = strata(&["--file", "e.strata", "render"], &d, None);
    assert!(r1.contains(r#""a\tb\\n""#), "{r1}");
    let (_, out) = strata(&["--file", "e.strata", "run", "s"], &d, None);
    assert!(out.contains(r#""a\tb\\n""#), "{out}");
}

const SHAPES: &str = "pub type Shape =\n  | Circle { radius: Int }\n  | Square(Int)\n\npub type Line = { sku: String, qty: Int }\n\npub trait Sized {\n  fn size(self) -> Int\n}\n\nimpl Sized for Line {\n  fn size(self) -> Int {\n    self.qty\n  }\n}\n";
const SHAPES_MAIN: &str = "use shapes.*\n\nfn area(s: Shape) -> Int {\n  match s {\n    Circle { radius } => 3 * radius * radius,\n    Square(n) => n * n,\n  }\n}\n\nfn bigger(l: Line) -> Line {\n  Line { ..l, qty: l.qty + 1 }\n}\n\ntest \"shapes\" {\n  assert area(Circle { radius: 1 }) == 3\n  assert Square(2) is Square\n  l := Line { sku: \"a\", qty: 2 }\n  assert bigger(l).size() == 3\n  assert l.size() == 2\n}\n";

#[test]
fn rename_variants_methods_and_fields() {
    let d = tmp("members");
    assert!(strata(&["init"], &d, None).0);
    save(&d.join("shapes.strata"), SHAPES);
    save(&d.join("main.strata"), SHAPES_MAIN);

    // a variant: constructors, patterns and `is`
    let (ok, out) = strata(&["rename", "Circle", "Round"], &d, None);
    assert!(ok && out.contains("renamed shapes.Circle → shapes.Round") && out.contains("updated 2 definitions that refer to it: area, test \"shapes\""), "{out}");
    // a trait method: the trait, its impls and the calls
    let (ok, out) = strata(&["rename", "size", "measure"], &d, None);
    assert!(ok && out.contains("shapes.impl Sized for Line"), "{out}");
    // a field: the declaration, accesses, literals, updates
    let (ok, out) = strata(&["rename", "Line.qty", "quantity"], &d, None);
    assert!(ok && out.contains("renamed shapes.Line.qty → shapes.Line.quantity"), "{out}");
    let (s, m) = (text(&d.join("shapes.strata")), text(&d.join("main.strata")));
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
    save(&d.join("more.strata"), "pub type Order = { sku: String, total: Int }\n\ndistinct type Id = String\n");
    // a field another record also has: the checker tells their accesses apart
    let (ok, out) = strata(&["rename", "Line.sku", "code"], &d, None);
    assert!(ok, "{out}");
    assert!(text(&d.join("more.strata")).contains("pub type Order = { sku: String"), "Order keeps its field");
    // `value`, which distinct types also have
    save(&d.join("more.strata"), "pub type Order = { value: Int }\n\ndistinct type Id = String\n\nfn both(o: Order, i: Id) -> String {\n  f\"{o.value} {i.value}\"\n}\n");
    let (ok, out) = strata(&["rename", "Order.value", "amount"], &d, None);
    assert!(ok, "{out}");
    assert!(text(&d.join("more.strata")).contains("f\"{o.amount} {i.value}\""), "the distinct type's `.value` stays");
    let (_, log) = strata(&["log", "--json"], &d, None);
    assert!(log.contains(r#""kind":"variant""#) && log.contains(r#""kind":"method""#) && log.contains(r#""kind":"field""#), "{log}");
}

#[test]
fn sync_follows_member_renames() {
    let d = tmp("follow-members");
    git(&d, &["init", "-q", "-b", "main"]);
    assert!(strata(&["init"], &d, None).0);
    save(&d.join("shapes.strata"), SHAPES);
    save(&d.join("main.strata"), SHAPES_MAIN);
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "base"]);

    // feature renames a variant and a field; main adds code that uses the old names
    git(&d, &["checkout", "-q", "-b", "feature"]);
    assert!(strata(&["rename", "Circle", "Round"], &d, None).0);
    assert!(strata(&["rename", "Line.qty", "quantity"], &d, None).0);
    git(&d, &["checkout", "-q", "main"]);
    save(&d.join("extra.strata"), "use shapes.*\n\nfn unit() -> Shape {\n  Circle { radius: 1 }\n}\n\nfn doubled(l: Line) -> Int {\n  l.qty * 2\n}\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "extra"]);
    git(&d, &["checkout", "-q", "feature"]);

    let (ok, out) = strata_git(&["sync", "main"], &d);
    assert!(ok && out.contains("merged main"), "{out}");
    let e = text(&d.join("extra.strata"));
    assert!(e.contains("Round { radius: 1 }") && e.contains("l.quantity * 2"), "{e}");
}

#[test]
fn rename_a_field_that_another_record_also_has() {
    let d = tmp("shared-field");
    let src = "type Line = { sku: String, qty: Int }\n\ntype Order = { sku: String, lines: List[Line] }\n\nfn label(l: Line, o: Order) -> String {\n  l.sku + \"/\" + o.sku\n}\n\nfn first_skus(o: Order) -> List[String] {\n  o.lines.map(l -> l.sku)\n}\n\nfn fix(l: Line) -> Line {\n  match l {\n    Line { sku, .. } => Line { ..l, sku: sku + \"!\" },\n  }\n}\n\ntest \"skus\" {\n  l := Line { sku: \"a\", qty: 1 }\n  o := Order { sku: \"o\", lines: [l] }\n  assert label(l, o) == \"a/o\"\n  assert first_skus(o) == [\"a\"]\n  assert fix(l).sku == \"a!\"\n}\n";
    assert!(strata(&["init"], &d, None).0);
    save(&d.join("main.strata"), src);
    // the checker says which record each `.sku` reads: Line's are renamed, Order's stay
    let (ok, out) = strata(&["rename", "Line.sku", "code"], &d, None);
    assert!(ok, "{out}");
    let m = text(&d.join("main.strata"));
    for x in ["type Line = { code: String, qty: Int }", "type Order = { sku: String", "l.code + \"/\" + o.sku", "o.lines.map(l -> l.code)", "Line { code: sku, .. } => Line { ..l, code: sku + \"!\" }", "Line { code: \"a\", qty: 1 }", "Order { sku: \"o\"", "fix(l).code == \"a!\""] {
        assert!(m.contains(x), "missing {x:?} in\n{m}");
    }
    let (ok, out) = strata(&["test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");

    // read from a union of both records: renaming one would break it
    let e = tmp("shared-field-union");
    assert!(strata(&["init"], &e, None).0);
    save(&e.join("main.strata"), "type A = { tag: String, n: Int }\n\ntype B = { tag: String }\n\nfn tag_of(x: A | B) -> String {\n  x.tag\n}\n");
    let (ok, out) = strata(&["rename", "A.tag", "label"], &e, None);
    assert!(!ok && out.contains("in `tag_of`, `.tag` is read from a union that also has `B.tag` (rename it too, in the same command)"), "{out}");
    // both records' fields to different names: the union access can't follow
    let (ok, out) = strata(&["rename", "A.tag", "label", "B.tag", "name"], &e, None);
    assert!(!ok && out.contains("`.tag` is read from a union whose records would get different names"), "{out}");
    // both, to the same name, in one step
    let (ok, out) = strata(&["rename", "A.tag", "label", "B.tag", "label"], &e, None);
    assert!(ok && out.contains("renamed A.tag → A.label, B.tag → B.label") && out.contains("updated 1 definition that refers to them: tag_of"), "{out}");
    let m = text(&e.join("main.strata"));
    assert!(m.contains("type A = { label: String, n: Int }") && m.contains("type B = { label: String }") && m.contains("x.label"), "{m}");
    let log = std::fs::read_to_string(e.join("strata.log")).unwrap();
    assert_eq!(log.lines().count(), 1, "one step");
}

#[test]
fn move_a_definition_to_another_module() {
    let d = tmp("move");
    assert!(strata(&["init"], &d, None).0);
    save(&d.join("orders.strata"), "pub type Order = { id: String, total: Int }\n\nfn doubled(o: Order) -> Int {\n  o.total * 2\n}\n\npub fn charge(o: Order) -> Int {\n  o.total + 1\n}\n\npub fn ship(o: Order) -> Int {\n  charge(o) + doubled(o)\n}\n");
    save(&d.join("billing.strata"), "use orders.{Order, charge}\n\npub fn invoice(o: Order) -> Int {\n  charge(o) + orders.ship(o)\n}\n");
    save(&d.join("main.strata"), "use orders.*\n\ntest \"charges\" {\n  o := Order { id: \"a\", total: 1 }\n  assert charge(o) == 2\n  assert orders.charge(o) == 2\n  assert ship(o) == 4\n}\n");

    let (ok, out) = strata(&["move", "charge", "billing"], &d, None);
    assert!(ok && out.contains("moved orders.charge from orders to billing (billing.strata)"), "{out}");
    assert!(out.contains("updated 3 definitions that refer to it: billing.invoice, test \"charges\", orders.ship") && out.contains("orders: use billing.charge") && out.contains("main: use billing.charge"), "{out}");
    let (o, b, m) = (text(&d.join("orders.strata")), text(&d.join("billing.strata")), text(&d.join("main.strata")));
    assert!(o.starts_with("use billing.charge\n") && !o.contains("fn charge"), "{o}");
    assert!(b.starts_with("use orders.Order\n") && b.contains("pub fn charge(o: Order) -> Int"), "{b}");
    // `orders.charge(o)` was stored as a reference to the definition: now `billing.charge`, shown short since imported
    assert!(m.contains("use billing.charge") && m.contains("assert charge(o) == 2") && !m.contains("orders.charge"), "{m}");
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
    assert!(strata(&["init"], &d, None).0);
    save(&d.join("orders.strata"), "pub type Order = { id: String, total: Int }\n\npub fn charge(o: Order) -> Int {\n  o.total + 1\n}\n");
    save(&d.join("billing.strata"), "use orders.Order\n\npub fn invoice(o: Order) -> Int {\n  o.total\n}\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "base"]);

    // feature moves `charge`; main adds code that calls it where it was
    git(&d, &["checkout", "-q", "-b", "feature"]);
    assert!(strata(&["move", "charge", "billing"], &d, None).0);
    git(&d, &["checkout", "-q", "main"]);
    save(&d.join("report.strata"), "use orders.{Order, charge}\n\nfn twice(o: Order) -> Int {\n  charge(o) + orders.charge(o)\n}\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "report"]);
    git(&d, &["checkout", "-q", "feature"]);

    let (ok, out) = strata_git(&["sync", "main"], &d);
    assert!(ok && out.contains("merged main") && out.contains("follows the rename orders.charge → billing.charge"), "{out}");
    // both calls name the same definition: stored alike, shown short where it is imported
    let r = text(&d.join("report.strata"));
    assert!(r.contains("use orders.Order") && r.contains("use billing.charge") && r.contains("charge(o) + charge(o)"), "{r}");
}

const TAXES: &str = "fn tax(amount: Int) -> Int {\n  amount / 10\n}\n\nfn total(amount: Int) -> Int {\n  amount + tax(amount)\n}\n\nfn double_tax(a: Int) -> Int {\n  tax(a) * 2\n}\n\ntest \"totals\" {\n  assert total(100) == 110\n}\n";

#[test]
fn a_breaking_edit_opens_a_pending_change_with_holes() {
    let d = tmp("pending");
    assert!(strata(&["init"], &d, None).0);
    save(&d.join("main.strata"), TAXES);

    // the edit itself checks; the uses it breaks become holes that remember what they were
    let (ok, out) = strata(&["edit", "-m", "tax takes a rate"], &d, Some("fn tax(amount: Int, rate: Int) -> Int { amount * rate / 100 }"));
    assert!(ok && out.contains("the edit breaks 2 uses elsewhere"), "{out}");
    assert!(out.contains("?pending_1  in total\n      was: tax(amount)\n      why: call to `tax`: expects 2 arguments, got 1"), "{out}");
    let m = text(&d.join("main.strata"));
    assert!(m.contains("amount + ?pending_1") && m.contains("?pending_2 * 2"), "{m}");
    assert_eq!(std::fs::read_to_string(d.join("strata.log")).unwrap(), "", "nothing logged yet");
    let (_, out) = strata(&["holes"], &d, None);
    assert!(out.contains("?pending_1  in total\n  goal:    Int\n  was:     tax(amount)"), "{out}");
    // a pending change's holes don't get merged
    let (ok, out) = strata(&["ci"], &d, None);
    assert!(!ok && out.contains("?pending_1 in total belongs to an unfinished change"), "{out}");

    // an edit that doesn't check in itself is still rejected
    let (ok, out) = strata(&["edit"], &d, Some("fn total(amount: Int) -> Int { amount + true }"));
    assert!(!ok && out.contains("rejected"), "{out}");

    // fill one, rename along the way (it joins the change), and `done` waits for the other
    assert!(strata(&["edit"], &d, Some("fn total(amount: Int) -> Int { amount + tax(amount, 10) }")).0);
    assert!(strata(&["rename", "double_tax", "twice_tax"], &d, None).0);
    let (ok, out) = strata(&["status"], &d, None);
    assert!(ok && out.contains("pending change: tax takes a rate  (1 hole left, 1 filled)") && out.contains("?pending_2  in twice_tax"), "{out}");
    let (ok, out) = strata(&["done"], &d, None);
    assert!(!ok && out.contains("1 hole of this change is still open"), "{out}");
    assert!(strata(&["edit"], &d, Some("fn twice_tax(a: Int) -> Int { tax(a, 10) * 2 }")).0);
    let (ok, out) = strata(&["done"], &d, None);
    assert!(ok && out.contains("recorded \"tax takes a rate\" as one change"), "{out}");

    // one log entry for the whole change, with its rename
    let log = std::fs::read_to_string(d.join("strata.log")).unwrap();
    assert_eq!(log.lines().count(), 1, "{log}");
    assert!(log.contains(r#""from":"double_tax""#), "{log}");
    let (ok, out) = strata(&["test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");
    let (_, out) = strata(&["status"], &d, None);
    assert!(out.contains("no pending change"), "{out}");
}

#[test]
fn a_pending_change_can_be_aborted_or_kept_with_its_holes() {
    let d = tmp("pending-abort");
    assert!(strata(&["init"], &d, None).0);
    save(&d.join("main.strata"), TAXES);
    assert!(strata(&["edit"], &d, Some("fn tax(amount: Int, rate: Int) -> Int { amount * rate / 100 }")).0);
    assert!(strata(&["edit", "extra"], &d, Some("fn more() -> Int { 1 }")).0);
    assert!(d.join("extra.strata").exists());
    // abort: every file as it was, including the module the change added
    let (ok, out) = strata(&["abort"], &d, None);
    assert!(ok && out.contains("undid the pending change"), "{out}");
    assert_eq!(text(&d.join("main.strata")), TAXES);
    assert!(!d.join("extra.strata").exists());
    assert_eq!(std::fs::read_to_string(d.join("strata.log")).unwrap(), "");

    // keep: recorded as is, the holes become ordinary ones
    assert!(strata(&["edit"], &d, Some("fn tax(amount: Int, rate: Int) -> Int { amount * rate / 100 }")).0);
    let (ok, out) = strata(&["done", "--keep-holes", "-m", "rates, callers to do"], &d, None);
    assert!(ok && out.contains("2 holes kept as ?todo_"), "{out}");
    let m = text(&d.join("main.strata"));
    assert!(m.contains("amount + ?todo_1") && m.contains("?todo_2 * 2"), "{m}");
    let (ok, out) = strata(&["ci"], &d, None);
    assert!(ok, "ordinary holes are fine: {out}");
}

/// A stand-in editor: on its n-th run it keeps what it was shown as
/// `seen<n>.strata` and saves `v<n>.strata` (or leaves the file as it is).
fn fake_editor(d: &std::path::Path, versions: &[&str]) -> String {
    for (i, v) in versions.iter().enumerate() {
        std::fs::write(d.join(format!("v{}.st", i + 1)), v).unwrap();
    }
    let script = d.join("editor.sh");
    let dir = d.display();
    std::fs::write(&script, format!("#!/bin/sh\nn=$(cat '{dir}/count' 2>/dev/null || echo 0); n=$((n+1)); echo $n > '{dir}/count'\ncp \"$1\" '{dir}/seen'$n.st\nif [ -f '{dir}/v'$n.st ]; then cp '{dir}/v'$n.st \"$1\"; fi\n")).unwrap();
    format!("sh '{}'", script.display())
}

fn strata_with_editor(args: &[&str], dir: &std::path::Path, editor: &str) -> (bool, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_strata")).args(args).current_dir(dir).env("EDITOR", editor).env_remove("VISUAL").stdin(Stdio::null()).output().unwrap();
    (o.status.success(), String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr))
}

#[test]
fn renames_leave_string_literals_alone() {
    // a string equal to the old name is text, not a reference: the rename goes ahead and keeps it
    let d = tmp("rename-literal");
    assert!(strata(&["init"], &d, None).0);
    save(&d.join("main.strata"), "fn greet() -> String {\n  \"hello\"\n}\n\nfn label() -> String {\n  \"greet\"\n}\n");
    let (ok, out) = strata(&["rename", "greet", "welcome", "--no-commit"], &d, None);
    assert!(ok, "{out}");
    let src = text(&d.join("main.strata"));
    assert!(src.contains("fn welcome()") && src.contains("\"greet\""), "{src}");
}

#[test]
fn edit_opens_the_code_in_an_editor() {
    let d = tmp("editor");
    let pkg = d.join("pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    assert!(strata(&["init"], &pkg, None).0);
    save(&pkg.join("main.strata"), TAXES);

    // one definition: the editor shows it alone, and what is saved is the edit
    let ed = d.join("one");
    std::fs::create_dir_all(&ed).unwrap();
    let editor = fake_editor(&ed, &["fn tax(amount: Int) -> Int {\n  amount / 5\n}\n"]);
    let (ok, out) = strata_with_editor(&["edit", "tax", "--editor", "-m", "more tax"], &pkg, &editor);
    assert!(ok && out.contains("changed"), "{out}");
    assert_eq!(std::fs::read_to_string(ed.join("seen1.st")).unwrap(), "fn tax(amount: Int) -> Int {\n  amount / 10\n}\n");
    assert!(text(&pkg.join("main.strata")).contains("amount / 5"));
    assert!(std::fs::read_to_string(pkg.join("strata.log")).unwrap().contains("more tax"));

    // an edit that doesn't check: the editor opens again with the errors on top
    let ed = d.join("retry");
    std::fs::create_dir_all(&ed).unwrap();
    let editor = fake_editor(&ed, &["fn total(amount: Int) -> Int {\n  amount + true\n}\n", "fn total(amount: Int) -> Int {\n  amount + tax(amount) + 1\n}\n"]);
    let (ok, out) = strata_with_editor(&["edit", "total", "--editor"], &pkg, &editor);
    assert!(ok, "{out}");
    let second = std::fs::read_to_string(ed.join("seen2.st")).unwrap();
    assert!(second.starts_with("// ✗ error in total") && second.contains("// ✗ not applied") && second.contains("amount + true"), "{second}");
    assert!(text(&pkg.join("main.strata")).contains("amount + tax(amount) + 1"));

    // the whole module; a definition deleted from the buffer is removed
    let ed = d.join("module");
    std::fs::create_dir_all(&ed).unwrap();
    let without = text(&pkg.join("main.strata")).replace("fn double_tax(a: Int) -> Int {\n  tax(a) * 2\n}\n\n", "");
    let editor = fake_editor(&ed, &[&without]);
    let (ok, out) = strata_with_editor(&["edit", "--editor"], &pkg, &editor);
    assert!(ok && out.contains("removed"), "{out}");
    assert!(!text(&pkg.join("main.strata")).contains("double_tax"));

    // saved unchanged: nothing happens
    let ed = d.join("same");
    std::fs::create_dir_all(&ed).unwrap();
    let editor = fake_editor(&ed, &[]);
    let (ok, out) = strata_with_editor(&["edit", "--editor"], &pkg, &editor);
    assert!(ok && out.contains("no changes"), "{out}");
    assert_eq!(std::fs::read_to_string(pkg.join("strata.log")).unwrap().lines().count(), 3);
}

#[test]
fn edit_opens_modules_and_the_whole_package() {
    let d = tmp("editor-modules");
    let pkg = d.join("pkg");
    std::fs::create_dir_all(pkg.join("shop")).unwrap();
    assert!(strata(&["init"], &pkg, None).0);
    save(&pkg.join("main.strata"), "use shop.orders.{price}\n\nfn total() -> Int {\n  price() + 1\n}\n");
    save(&pkg.join("shop/orders.strata"), "pub fn price() -> Int {\n  10\n}\n");

    // the whole package: one file per module, given to the editor together
    let ed = d.join("whole");
    std::fs::create_dir_all(&ed).unwrap();
    let script = ed.join("editor.sh");
    let dir = ed.display();
    std::fs::write(&script, format!("#!/bin/sh\nfor f in \"$@\"; do echo \"$f\" >> '{dir}/files'; done\nfor f in \"$@\"; do case \"$f\" in */shop/orders.st) sed 's/10/12/' \"$f\" > \"$f.new\" && mv \"$f.new\" \"$f\";; esac; done\n")).unwrap();
    let (ok, out) = strata_with_editor(&["edit", "--editor"], &pkg, &format!("sh '{}'", script.display()));
    assert!(ok && out.contains("changed"), "{out}");
    let opened = std::fs::read_to_string(ed.join("files")).unwrap();
    assert!(opened.lines().count() == 2 && opened.contains("main.st") && opened.contains("shop/orders.st"), "{opened}");
    assert!(text(&pkg.join("shop/orders.strata")).contains("12"));

    // a module by name: only its file
    let ed = d.join("module");
    std::fs::create_dir_all(&ed).unwrap();
    let editor = fake_editor(&ed, &["pub fn price() -> Int {\n  15\n}\n"]);
    let (ok, out) = strata_with_editor(&["edit", "shop.orders", "--editor"], &pkg, &editor);
    assert!(ok, "{out}");
    assert_eq!(std::fs::read_to_string(ed.join("seen1.st")).unwrap(), "pub fn price() -> Int {\n  12\n}\n");
    assert!(text(&pkg.join("shop/orders.strata")).contains("15"));

    // a new module, by its file
    let ed = d.join("new");
    std::fs::create_dir_all(&ed).unwrap();
    let editor = fake_editor(&ed, &["pub fn tax() -> Int {\n  2\n}\n"]);
    let (ok, out) = strata_with_editor(&["edit", "shop/tax.strata", "--editor"], &pkg, &editor);
    assert!(ok && out.contains("added"), "{out}");
    assert!(text(&pkg.join("shop/tax.strata")).contains("pub fn tax"));

    // `main` is the root module; `main.total` is its definition `total`
    let ed = d.join("root-def");
    std::fs::create_dir_all(&ed).unwrap();
    let editor = fake_editor(&ed, &[]);
    let (ok, out) = strata_with_editor(&["edit", "main.total", "--editor"], &pkg, &editor);
    assert!(ok && out.contains("no changes"), "{out}");
    assert_eq!(std::fs::read_to_string(ed.join("seen1.st")).unwrap(), "fn total() -> Int {\n  price() + 1\n}\n");
    let ed = d.join("root-module");
    std::fs::create_dir_all(&ed).unwrap();
    let editor = fake_editor(&ed, &[]);
    assert!(strata_with_editor(&["edit", "main", "--editor"], &pkg, &editor).0);
    assert!(std::fs::read_to_string(ed.join("seen1.st")).unwrap().starts_with("use shop.orders"));

    // neither a module nor a definition
    let (ok, out) = strata_with_editor(&["edit", "nothing_here", "--editor"], &pkg, "true");
    assert!(!ok && out.contains("is no module"), "{out}");
}

#[test]
fn lambda_parameter_refinements_hold_at_runtime() {
    let d = tmp("lambda-refine");
    save(&d.join("l.strata"),
        "fn apply(f: (Int) -> Int, x: Int) -> Int {\n  f(x)\n}\n\nfn bad() -> Int {\n  apply((n: Positive) -> n * 2, -3)\n}\n\nfn good() -> Int {\n  apply((n: Positive) -> n * 2, 3)\n}\n");
    let (ok, out) = strata(&["--file", "l.strata", "run", "good"], &d, None);
    assert!(ok && out.trim() == "6", "{out}");
    let (ok, out) = strata(&["--file", "l.strata", "run", "bad"], &d, None);
    assert!(!ok && out.contains("argument `n` of a lambda: -3 does not satisfy `Positive`"), "{out}");
}

#[test]
fn f_string_format_specs() {
    let d = tmp("fmt-spec");
    save(&d.join("f.strata"),
        "type Cents = Int where it >= 0\n\nfn row(name: String, price: Float, qty: Cents, delta: Int) -> String {\n  f\"[{name:<8}|{price:>8.2}|{qty:03}|{delta:+}|{name:*^10}|{name:.3}|{len([1, 2].map((n: Int) -> n))}]\"\n}\n\ntest \"specs\" {\n  assert row(\"tea\", 3.14159, 7, 5) == \"[tea     |    3.14|007|+5|***tea****|tea|2]\"\n  assert row(\"biscuits\", -2.5, 42, -3) == \"[biscuits|   -2.50|042|-3|*biscuits*|bis|2]\"\n  assert f\"{-42:06}\" == \"-00042\"\n}\n");
    let (ok, out) = strata(&["--file", "f.strata", "test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");
    // specs survive rendering
    let (_, r) = strata(&["--file", "f.strata", "render"], &d, None);
    assert!(r.contains("{price:>8.2}") && r.contains("{name:*^10}") && r.contains("{qty:03}"), "{r}");
    save(&d.join("e.strata"), "fn a(n: Int) -> String { f\"{n:.2}\" }\nfn b(s: String) -> String { f\"{s:05}\" }\n");
    let (ok, out) = strata(&["--file", "e.strata", "check"], &d, None);
    assert!(!ok && out.contains("`.N` applies to a Float") && out.contains("`+` and `0` apply to numbers"), "{out}");
    // a spec that doesn't parse can't be stored
    let (ok, out) = strata(&["--file", "x.strata", "edit"], &d, Some("fn c(n: Int) -> String { f\"{n:x}\" }\n"));
    assert!(!ok && out.contains("bad format spec `x`"), "{out}");
}

#[test]
fn a_types_own_methods_and_shared_names() {
    let d = tmp("own-methods");
    save(&d.join("o.strata"), r#"trait Describe {
  fn describe(self) -> String
    ensures it != ""
}

trait Log {
  fn describe(self) -> String
}

type Item = { name: NonEmptyString, cents: Natural }

impl Item {
  fn new(name: NonEmptyString, cents: Natural) -> Item {
    Item { name, cents }
  }

  fn describe(self) -> String {
    f"own {self.name}"
  }
}

impl Describe for Item {
  fn describe(self) -> String {
    f"describe {self.name}"
  }
}

impl Log for Item {
  fn describe(self) -> String {
    f"log {self.name}"
  }
}

type Sku = { code: NonEmptyString }

impl Describe for Sku {
  fn describe(self) -> String {
    "SKU " + self.code
  }
}

fn show(d: Describe) -> String {
  d.describe()
}

fn shout[T: Log](x: T) -> String {
  to_upper(x.describe())
}

fn all_of(xs: List[Describe]) -> List[String] {
  xs.map(x -> x.describe())
}

test "which describe runs" {
  i := Item.new("tea", 3)
  assert i.describe() == "own tea"
  assert show(i) == "describe tea"
  assert shout(i) == "LOG TEA"
  assert all_of([i]) == ["describe tea"]
  assert show(Sku { code: "AB" }) == "SKU AB"
  assert describe(Sku { code: "X" }) == "SKU X"
}
"#);
    // a call through a trait runs that trait's implementation; on a known type, the type's own method
    let (ok, out) = strata(&["--file", "o.strata", "test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");
    // each implementation is verified on its own, under its own label
    let (_, out) = strata(&["--file", "o.strata", "verify"], &d, None);
    for label in ["Item.new ", "Item.describe ", "Item.describe (Describe)", "Item.describe (Log)"] {
        assert!(out.contains(label), "missing {label:?} in {out}");
    }
    assert!(!out.contains('✗'), "{out}");
    let (_, r) = strata(&["--file", "o.strata", "render"], &d, None);
    assert_eq!(r, text(&d.join("o.strata")));

    save(&d.join("bad.strata"), r#"trait A {
  fn tag(self) -> String
}

trait B {
  fn tag(self) -> String
}

type P = { x: Int }

impl A for P {
  fn tag(self) -> String {
    "a"
  }
}

impl B for P {
  fn tag(self) -> String {
    "b"
  }
}

type Q = { y: Int }

impl Q {
  fn tag(self) -> String {
    "q"
  }
}

impl A for Q {
  fn tag(self) -> String {
    "qa"
  }
}

impl B for Q {
  fn tag(self) -> String {
    "qb"
  }
}

fn concrete_without_own(p: P) -> String {
  p.tag()
}

fn concrete_with_own(q: Q) -> String {
  q.tag()
}

fn through_a(x: A) -> String {
  x.tag()
}

fn either(x: A | B) -> String {
  x.tag()
}

fn both[T: A + B](x: T) -> String {
  x.tag()
}

fn as_value(q: Q) -> List[String] {
  [q].map(tag)
}
"#);
    let (ok, out) = strata(&["--file", "bad.strata", "check"], &d, None);
    assert!(!ok, "{out}");
    for want in [
        "error in concrete_without_own: `P` implements `A` and `B`, which both have `tag`: define `tag` in `impl P`",
        "error in either: `tag` is ambiguous here: the value is a `A | B`",
        "error in both: `tag` is ambiguous here: `T` is known only as `A` and `B`",
        "error in as_value: which `tag` runs depends on the type of the value",
    ] {
        assert!(out.contains(want), "missing {want:?} in {out}");
    }
    // a type's own method on a known type, and a call through one trait, are fine
    assert!(!out.contains("concrete_with_own") && !out.contains("through_a"), "{out}");
    assert_eq!(out.matches("error in").count(), 4, "{out}");

    save(&d.join("misc.strata"), "type Q = { y: Int }\n\nimpl Int {\n  fn twice(self) -> Int {\n    self * 2\n  }\n}\n\nfn g() -> Int {\n  Q.nope()\n}\n");
    let (ok, out) = strata(&["--file", "misc.strata", "check"], &d, None);
    assert!(!ok && out.contains("`impl Int`: methods can only be added to a record, sum or `distinct` type") && out.contains("`Q` has no method `nope` called on the type"), "{out}");
}

#[test]
fn trait_methods_without_self() {
    let d = tmp("static-methods");
    // `Money.empty()`, `T.empty()` (the type passed when the generic function runs), `Self.empty()` in a default method and a law
    save(&d.join("m.strata"), r#"trait Monoid {
  fn empty() -> Self
  fn combine(self, other: Self) -> Self
  fn twice(self) -> Self {
    self.combine(self).combine(Self.empty())
  }
  law left_identity(a: Self):
    Self.empty().combine(a) == a
}

type Money = { cents: Int }

impl Monoid for Money {
  fn empty() -> Money {
    Money { cents: 0 }
  }

  fn combine(self, other: Money) -> Money {
    Money { cents: self.cents + other.cents }
  }
}

impl Monoid for String {
  fn empty() -> String {
    ""
  }

  fn combine(self, other: String) -> String {
    self + other
  }
}

fn concat[T: Monoid](xs: List[T]) -> T {
  fold(xs, T.empty(), (acc, x) -> acc.combine(x))
}

fn concat_twice[T: Monoid](xs: List[T]) -> T {
  concat(xs).twice()
}

fn total(ms: List[Money]) -> Money {
  concat(ms)
}

test "static trait methods" {
  assert Money.empty() == Money { cents: 0 }
  assert total([Money { cents: 2 }, Money { cents: 3 }]) == Money { cents: 5 }
  assert concat(["a", "b"]) == "ab"
  let none: List[Money] = []
  assert concat(none) == Money { cents: 0 }
  assert concat_twice(["x"]) == "xx"
  assert String.empty() == ""
}
"#);
    let (ok, out) = strata(&["--file", "m.strata", "test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");
    let (ok, out) = strata(&["--file", "m.strata", "verify"], &d, None);
    assert!(ok && out.contains("law left_identity for Money") && out.contains("law left_identity for String") && !out.contains('✗'), "{out}");
    let (_, r) = strata(&["--file", "m.strata", "render"], &d, None);
    assert!(r.contains("fn empty() -> Self") && r.contains("T.empty()") && r.contains("Self.empty().combine(a) == a"), "{r}");

    // contracts of static methods are used by the solver; a law catches a wrong `empty`
    save(&d.join("s.strata"), r#"trait Bounded {
  fn lowest() -> Self
  fn rank(self) -> Int
    ensures it >= 0
}

trait Zero {
  fn zero() -> Self
    ensures it == it
}

type Level = { n: Natural }

impl Bounded for Level {
  fn lowest() -> Level {
    Level { n: 0 }
  }

  fn rank(self) -> Int {
    self.n
  }
}

fn lowest_rank[T: Bounded]() -> Natural {
  T.lowest().rank()
}

fn level_floor() -> Natural {
  Level.lowest().rank()
}

trait Monoid {
  fn empty() -> Self
  fn combine(self, other: Self) -> Self
  law left_identity(a: Self):
    Self.empty().combine(a) == a
}

type Max = { v: Int }

impl Monoid for Max {
  fn empty() -> Max {
    Max { v: 5 }
  }

  fn combine(self, other: Max) -> Max {
    if self.v > other.v { self } else { other }
  }
}

test "generic with no arguments" {
  assert level_floor() == 0
}
"#);
    let (_, out) = strata(&["--file", "s.strata", "verify"], &d, None);
    assert!(passed(&out, "lowest_rank") && passed(&out, "level_floor") && out.contains("✗ law left_identity for Max"), "{out}");
    save(&d.join("p.strata"), r#"trait Parse {
  fn parse(s: String) -> Self
    requires s != ""
  fn size(self) -> Int
    ensures it >= 0
}

type Word = { text: String }

impl Parse for Word {
  fn parse(s: String) -> Word {
    Word { text: s }
  }

  fn size(self) -> Int {
    len(self.text.code_points())
  }
}

fn read[T: Parse](s: NonEmptyString) -> Natural {
  T.parse(s).size()
}

fn word_size(s: NonEmptyString) -> Natural {
  Word.parse(s).size()
}

test "parse" {
  assert word_size("hello") == 5
}
"#);
    let (ok, out) = strata(&["--file", "p.strata", "verify"], &d, None);
    assert!(ok && passed(&out, "read") && passed(&out, "word_size"), "{out}");

    save(&d.join("bad.strata"), r#"trait Monoid {
  fn empty() -> Self
  fn combine(self, other: Self) -> Self
}

trait Version {
  fn version() -> Int
}

type Money = { cents: Int }

impl Monoid for Money {
  fn empty() -> Money {
    Money { cents: 0 }
  }

  fn combine(self, other: Money) -> Money {
    Money { cents: self.cents + other.cents }
  }
}

fn concat[T: Monoid](xs: List[T]) -> T {
  fold(xs, T.empty(), (acc, x) -> acc.combine(x))
}

fn unbounded[T](x: T) -> T {
  T.empty()
}

fn as_value(xss: List[List[Money]]) -> List[Money] {
  xss.map(concat)
}

fn not_impl() -> Int {
  Int.empty()
}

test "unknown type" {
  assert concat([]) == concat([])
}
"#);
    let (ok, out) = strata(&["--file", "bad.strata", "check"], &d, None);
    assert!(!ok, "{out}");
    for want in [
        "trait method `version` has no `self` and doesn't mention `Self`",
        "error in unbounded: `T` has no method `empty` called on the type",
        "error in as_value: `concat` needs to know its type arguments when it runs",
        "error in not_impl: calling `empty`: `Int` does not implement `Monoid`",
        "could not work out which type must implement `Monoid`; add a type annotation",
    ] {
        assert!(out.contains(want), "missing {want:?} in {out}");
    }
}

#[test]
fn print_goes_to_the_terminal_a_test_buffer_or_nowhere() {
    let d = tmp("print");
    save(&d.join("p.strata"),
        r#"use std.io.Console

fn double(n: Int) -> Int {
  print(f"doubling {n}")
  n * 2
}

fn main(mut console: Console) {
  print("plain")
  print([1, 2])
  mut console.print("console")
  print(double(21))
}

test "passes" {
  assert double(1) == 2
}

test "fails" {
  assert double(2) == 5
}
"#);
    // `print` is the prelude's even where `Console` (with its `print` method) is imported
    let (code, out, _) = strata_code(&["run", "p.strata"], &d, "");
    assert_eq!((code, out.as_str()), (0, "plain\n[1, 2]\nconsole\ndoubling 21\n42\n"));
    // a test's output is shown only when it fails, once
    let (_, out) = strata(&["--file", "p.strata", "test"], &d, None);
    assert!(out.contains("✓ passes") && !out.contains("doubling 1"), "{out}");
    assert_eq!(out.matches("doubling 2").count(), 1, "{out}");
    // verify discards it
    let (_, out, err) = strata_code(&["--file", "p.strata", "verify", "--cases", "20"], &d, "");
    assert!(!out.contains("doubling") && !err.contains("doubling"), "{out}{err}");
}

#[test]
fn command_line_options_are_read_into_a_record() {
    let d = tmp("cli");
    std::fs::create_dir_all(d.join("tool/kinds")).unwrap();
    std::fs::write(d.join("tool/strata.toml"), "name = \"tool\"\n").unwrap();
    save(&d.join("tool/kinds.strata"), "pub type Mode =\n  | Fast\n  | DryRun\n");
    save(&d.join("tool/main.strata"),
        r#"use std.cli.CommandLine
use kinds.*

/// Copy things.
type Options = {
  /// Where from.
  source: String,
  /// How many copies.
  copies: Positive,
  /// Say more.
  verbose: Bool,
  /// How to go about it.
  mode: Option[Mode],
  /// A ratio.
  ratio: Option[Float where 0.0 <= it && it <= 1.0],
  extra: List[Int],
}

derive CommandLine for Options

fn main(opts: Options) {
  print(opts)
}
"#);
    let run = |args: &[&str]| {
        let mut a = vec!["run", "tool", "--"];
        a.extend_from_slice(args);
        strata_code(&a, &d, "")
    };
    let (code, out, err) = run(&["src", "3", "--verbose", "--mode", "dry-run", "--ratio=0.5", "7", "8"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains(r#"source: "src""#) && out.contains("copies: 3") && out.contains("verbose: true"), "{out}");
    assert!(out.contains("mode: Some(DryRun)") && out.contains("ratio: Some(0.5)") && out.contains("extra: [7, 8]"), "{out}");
    // defaults: a flag is false, an option None, the list empty
    let (code, out, _) = run(&["src", "1"]);
    assert!(code == 0 && out.contains("verbose: false") && out.contains("mode: None") && out.contains("extra: []"), "{out}");
    // mistakes exit 2 and name the argument
    for (args, want) in [
        (vec!["src"], "missing <copies>: How many copies."),
        (vec!["src", "0"], "<copies>: 0 does not satisfy"),
        (vec!["src", "x"], "<copies>: `x` is not a whole number"),
        (vec!["src", "1", "--mode", "slow"], "`slow` is not one of fast, dry-run"),
        (vec!["src", "1", "--ratio", "2.0"], "--ratio: 2.0 does not satisfy"),
        (vec!["src", "1", "--colour"], "unknown option `--colour`"),
        (vec!["src", "1", "--verbose", "--verbose"], "given twice"),
        (vec!["src", "1", "--verbose=yes"], "takes no value"),
        (vec!["src", "1", "x"], "[extra...]: `x` is not a whole number"),
    ] {
        let (code, out, err) = run(&args);
        assert!(code == 2 && out.is_empty() && err.contains(want) && err.contains("tool --help"), "{args:?}: {err}");
    }
    // `--` ends the options
    let (code, out, _) = run(&["--", "--verbose", "2"]);
    assert!(code == 0 && out.contains(r#"source: "--verbose""#), "{out}");
    // help comes from the doc comments
    let (code, out, _) = run(&["--help"]);
    assert_eq!(code, 0);
    for want in ["Copy things.", "Usage: tool [OPTIONS] <source> <copies> [extra...]", "<copies>", "How many copies.", "--mode <fast|dry-run>", "--ratio <Float>", "(where 0.0 <= it && it <= 1.0)", "--verbose", "-h, --help"] {
        assert!(out.contains(want), "missing {want:?} in\n{out}");
    }
    // a record that can't be read from arguments is a check error
    save(&d.join("bad.strata"), "use std.cli.CommandLine\n\ntype Opts = { pairs: List[(Int, Int)] }\n\nderive CommandLine for Opts\n");
    let (ok, out) = strata(&["--file", "bad.strata", "check"], &d, None);
    assert!(!ok && out.contains("`Opts.pairs` is a `List[(Int, Int)]`"), "{out}");
    // field docs round-trip
    let (_, r1) = strata(&["--file", "tool", "render", "main"], &d, None);
    assert!(r1.contains("  /// How many copies.\n  copies: Positive,"), "{r1}");
}

#[test]
fn method_names_follow_the_receiver_across_modules_and_builtins() {
    let d = tmp("names");
    // `Console.write` / `Files.write` from two modules, `Files.remove` / the builtin `remove`
    save(&d.join("main.strata"),
        r#"use std.io.{Console, recorder, printed}
use std.fs.{Disk, Files, NotFound, mem_files}

fn save(mut console: Console, mut files: Files) {
  mut files.write("a.txt", "hi") except { () }
  mut console.write("saved ")
  mut console.print(files.read("a.txt") except { "?" })
  mut files.remove("a.txt") except { () }
  mut console.print(f"{files.exists("a.txt")}")
}

fn shout[C: Console](mut c: C, s: String) {
  mut c.write(to_upper(s))
  mut c.print("!")
}

fn main(mut console: Console, mut files: Disk) {
  mut files.write("x.txt", "disk") except { () }
  shout(mut console, files.read("x.txt") except { "?" })
  mut files.remove("x.txt") except { () }
}

test "each call means its receiver's method" {
  mut c := recorder([])
  mut f := mem_files([("b.txt", "x")])
  save(mut c, mut f)
  assert printed(c) == ["saved hi", "false"]
  shout(mut c, "yo")
  assert printed(c) == ["saved hi", "false", "YO!"]
  dict_ := dict([("k", 1), ("j", 2)])
  assert keys(dict_.remove("k")) == ["j"]
  assert keys(remove(dict_, "j")) == ["k"]
  assert to_list(set([1, 2]).remove(1)) == [2]
  assert f.list("nope") == Err(NotFound { path: "nope" })
}
"#);
    let (ok, out) = strata(&["--file", "main.strata", "test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");
    let (code, out, err) = strata_code(&["run", "main.strata"], &d, "");
    assert_eq!((code, out.as_str()), (0, "DISK!\n"), "{err}");
    assert!(!d.join("x.txt").exists());
    // verify picks a stand-in for `C: Console`, never the real terminal
    let (_, out, err) = strata_code(&["--file", "main.strata", "verify", "--cases", "20"], &d, "");
    assert!(!out.contains("needs the real system") && !out.contains("YO") && !err.contains("needs the real system"), "{out}{err}");
}

#[test]
fn shared_method_names_across_modules() {
    let d = tmp("xmod");
    std::fs::write(d.join("strata.toml"), "name = \"xmod\"\n").unwrap();
    save_module(&d, "a", "pub trait Sized {\n  fn size(self) -> Int\n}\n");
    save_module(&d, "b", "pub trait Weighed {\n  fn size(self) -> Int\n}\n");
    save_module(&d, "main",
        r#"use a.Sized
use b.Weighed

type Box = { n: Int }
type Bag = { n: Int }
type Both = { n: Int }

impl Sized for Box {
  fn size(self) -> Int { self.n }
}
impl Weighed for Bag {
  fn size(self) -> Int { self.n * 100 }
}
impl Both {
  fn size(self) -> Int { 7 }
}
impl Sized for Both {
  fn size(self) -> Int { 1 }
}
impl Weighed for Both {
  fn size(self) -> Int { 2 }
}

fn via_sized(x: Sized) -> Int { x.size() }
fn via_weighed[T: Weighed](x: T) -> Int { x.size() }

test "each call means its receiver's size" {
  assert Box { n: 3 }.size() == 3
  assert Bag { n: 3 }.size() == 300
  assert via_sized(Box { n: 4 }) == 4
  assert via_weighed(Bag { n: 4 }) == 400
  b := Both { n: 0 }
  assert b.size() == 7
  assert via_sized(b) == 1
  assert via_weighed(b) == 2
}
"#);
    let (ok, out) = strata(&["--file", ".", "test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");
    // two traits of the receiver have the name, from different modules: an error, as within one
    save_module(&d, "amb",
        r#"use a.Sized
use b.Weighed

type Two = { n: Int }
impl Sized for Two {
  fn size(self) -> Int { 1 }
}
impl Weighed for Two {
  fn size(self) -> Int { 2 }
}

fn concrete(x: Two) -> Int { x.size() }
fn bounded[T: Sized + Weighed](x: T) -> Int { x.size() }
"#);
    let (ok, out) = strata(&["--file", ".", "check"], &d, None);
    assert!(!ok && out.contains("`Two` implements `Sized` and `Weighed`, which both have `size`"), "{out}");
    assert!(out.contains("`T` is known only as `Sized` and `Weighed`"), "{out}");
}

#[test]
fn a_pending_change_handles_removed_and_added_variants() {
    let d = tmp("pending-variants");
    assert!(strata(&["init"], &d, None).0);
    save(&d.join("main.strata"),
        "type Status =\n  | Pending\n  | Shipped { tracking: String }\n  | Cancelled\n\nfn label(s: Status) -> String {\n  match s {\n    Pending => \"pending\",\n    Shipped { tracking, .. } => \"shipped \" + tracking,\n    Cancelled => \"cancelled\",\n  }\n}\n\nfn done(s: Status) -> Bool {\n  match s {\n    Shipped { .. } | Cancelled => true,\n    Pending => false,\n  }\n}\n\nfn only_cancelled(s: Status) -> Int {\n  match s {\n    Cancelled => 1,\n  }\n}\n\nfn cancel() -> Status {\n  Cancelled\n}\n\ntest \"labels\" {\n  assert label(Pending) == \"pending\"\n}\n");
    let (ok, out) = strata(&["edit", "-m", "lost parcels"], &d, Some("type Status =\n  | Pending\n  | Shipped { tracking: String }\n  | Lost(Int)\n"));
    assert!(ok, "{out}");
    // a removed variant's arms go (or just its alternative); an added one gets an arm with a hole
    assert!(out.contains("in label: dropped the arm for `Cancelled` (removed)") && out.contains("in done: dropped `Cancelled` (removed) from an arm"), "{out}");
    assert!(out.contains("in label\n      why: `Lost` is a new variant of `Status`"), "{out}");
    let m = text(&d.join("main.strata"));
    assert!(m.contains("    Shipped { tracking, .. } => \"shipped \" + tracking,\n    Lost(_) => ?pending_"), "{m}");
    assert!(m.contains("    Shipped { .. } => true,\n    Pending => false,\n    Lost(_) => ?pending_"), "{m}");
    // a match with no arm left is a hole; so is building the removed variant
    assert!(m.contains("fn only_cancelled(s: Status) -> Int\n  = ?pending_") && m.contains("fn cancel() -> Status\n  = ?pending_"), "{m}");
    let (_, out) = strata(&["holes"], &d, None);
    assert!(out.contains("was:     Cancelled"), "{out}");
    // filled, the change is one entry
    assert!(strata(&["edit"], &d, Some("fn label(s: Status) -> String {\n  match s {\n    Pending => \"pending\",\n    Shipped { tracking, .. } => \"shipped \" + tracking,\n    Lost(n) => f\"lost {n}\",\n  }\n}\n")).0);
    assert!(strata(&["edit"], &d, Some("fn done(s: Status) -> Bool {\n  match s {\n    Shipped { .. } | Lost(_) => true,\n    Pending => false,\n  }\n}\n")).0);
    assert!(strata(&["edit", "--rm", "only_cancelled", "--rm", "cancel"], &d, Some("")).0);
    let (ok, out) = strata(&["done"], &d, None);
    assert!(ok && out.contains("recorded \"lost parcels\" as one change"), "{out}");
    let (ok, out) = strata(&["test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");
}

#[test]
fn changes_are_committed_to_git() {
    let d = tmp("auto-commit");
    git(&d, &["init", "-q", "-b", "main"]);
    let pkg = d.join("pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    assert!(strata(&["init"], &pkg, None).0);
    save(&pkg.join("main.strata"), TAXES);
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "base"]);
    let count = || git(&d, &["rev-list", "--count", "HEAD"]).trim().parse::<usize>().unwrap();

    // a change that checks is a commit, with its message; what else is staged stays out
    std::fs::write(d.join("notes.txt"), "mine").unwrap();
    git(&d, &["add", "notes.txt"]);
    let (ok, out) = strata_git_stdin(&["edit", "-m", "cheaper tax"], &pkg, "fn tax(amount: Int) -> Int { amount / 20 }");
    assert!(ok && out.contains("committed to git:") && out.contains("cheaper tax"), "{out}");
    assert_eq!(count(), 2);
    assert_eq!(git(&d, &["log", "-1", "--format=%s"]).trim(), "cheaper tax");
    assert!(git(&d, &["status", "--porcelain"]).contains("A  notes.txt"), "still only staged");

    // a pending change is committed once, when it's done
    assert!(strata_git_stdin(&["edit", "-m", "tax takes a rate"], &pkg, "fn tax(amount: Int, rate: Int) -> Int { amount * rate / 100 }").0);
    assert!(strata_git_stdin(&["edit"], &pkg, "fn total(amount: Int) -> Int { amount + tax(amount, 10) }").0);
    assert!(strata_git_stdin(&["edit"], &pkg, "fn double_tax(a: Int) -> Int { tax(a, 10) * 2 }").0);
    assert_eq!(count(), 2, "nothing committed while pending");
    let (ok, out) = strata_git(&["done"], &pkg);
    assert!(ok && out.contains("committed to git:"), "{out}");
    assert_eq!(count(), 3);
    assert_eq!(git(&d, &["log", "-1", "--format=%s"]).trim(), "tax takes a rate");

    // off for one command, or for the package
    assert!(strata_git_stdin(&["edit", "--no-commit"], &pkg, "fn total(amount: Int) -> Int { amount + tax(amount, 12) }").0);
    assert_eq!(count(), 3);
    std::fs::write(pkg.join("strata.toml"), "name = \"pkg\"\nauto_commit = false\n").unwrap();
    assert!(strata_git_stdin(&["edit"], &pkg, "fn total(amount: Int) -> Int { amount + tax(amount, 15) }").0);
    assert_eq!(count(), 3);
}

fn strata_git_stdin(args: &[&str], dir: &std::path::Path, stdin: &str) -> (bool, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_strata"))
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    (o.status.success(), String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr))
}

#[test]
fn std_web_renders_and_proves_pages() {
    let ex = format!("{}/examples/page", env!("CARGO_MANIFEST_DIR"));
    let d = tmp("web");
    let (ok, out) = strata(&["--file", &ex, "test"], &d, None);
    assert!(ok && out.contains("6 passed"), "{out}");
    let (ok, out) = strata(&["run", &ex], &d, None);
    assert!(ok && out.starts_with("<!DOCTYPE html>") && out.contains("&lt;script&gt;alert(1)&lt;/script&gt;"), "text is escaped: {out}");
    assert!(out.contains(r#"<a href="/?sort=year">By year</a>"#), "{out}");
    // an inline script can't be an attribute: caught when checking
    save(&d.join("bad.strata"), "use std.web.{Html, render, attr, button}\nfn b() -> String { render(button([attr(\"onclick\", \"evil()\")], [])) }\n");
    let (ok, out) = strata(&["--file", "bad.strata", "check"], &d, None);
    assert!(!ok && out.contains("does not satisfy `std.web.AttrName`"), "{out}");
    // nor can `Html` be built around the functions that check names
    save(&d.join("sneak.strata"), "use std.web.{Html, render}\nfn b() -> String { render(Html { node: Element { tag: \"x y\", attrs: [], children: [] } }) }\n");
    let (ok, out) = strata(&["--file", "sneak.strata", "check"], &d, None);
    assert!(!ok && out.contains("`Element` is private to module `std.web`"), "{out}");
    if !has_solver() {
        eprintln!("z3 is not installed: skipping");
        return;
    }
    // names are checked where elements are made; `Html` itself has nothing to check
    let (ok, out) = strata(&["--file", &ex, "verify", "--force", "--require", "proven"], &d, None);
    assert!(ok && out.contains("✓ page") && out.contains("✓ row"), "{out}");
    // the router's handlers too: a lambda's request is valid, and `app` isn't recursive
    assert!(out.contains("✓ app") && out.contains("✓ language") && out.contains("✓ handle"), "{out}");
}

/// One HTTP request to a local server: the whole response as text.
fn http(port: u16, request: &str) -> String {
    use std::io::Read;
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(request.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

#[test]
fn serve_calls_handle_for_each_request() {
    let d = tmp("serve");
    // a free port: bound, then released for the server
    let port = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
    save(&d.join("app.strata"),
        r#"use std.http.{Request, Response, ok_text, ok_json, not_found}
fn handle(req: Request) -> Response {
  q := get(req.query, "q") or { "-" }
  match req.path {
    "/" => ok_text(f"{req.method} q={q} body={req.body} agent={get(req.headers, "x-agent") or { "-" }}"),
    "/json" => ok_json([1, 2]),
    "/crash" => ok_text(to_string(10 / len(req.body.code_points()))),
    "/inject" => Response { status: 200, headers: dict([("x-a", "b\r\nset-cookie: evil=1")]), body: "" },
    _ => not_found(),
  }
}
"#);
    let mut server = Command::new(env!("CARGO_BIN_EXE_strata")).args(["serve", "app.strata", "--port", &port.to_string()]).current_dir(&d).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let up = (0..100).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
    });
    let results = std::panic::catch_unwind(|| {
        assert!(up, "the server didn't start");
        let r = http(port, "POST /?q=a+b%21 HTTP/1.1\r\nHost: x\r\nX-Agent: t\r\nContent-Length: 5\r\n\r\nhello");
        assert!(r.starts_with("HTTP/1.1 200 OK") && r.ends_with("POST q=a b! body=hello agent=t"), "{r}");
        let r = http(port, "GET /json HTTP/1.1\r\n\r\n");
        assert!(r.contains("content-type: application/json") && r.ends_with("[1,2]"), "{r}");
        assert!(http(port, "GET /nope HTTP/1.1\r\n\r\n").starts_with("HTTP/1.1 404"));
        let r = http(port, "GET /crash HTTP/1.1\r\n\r\n");
        assert!(r.starts_with("HTTP/1.1 500") && r.contains("division by zero"), "a fault is a 500: {r}");
        let r = http(port, "GET /inject HTTP/1.1\r\n\r\n");
        assert!(r.starts_with("HTTP/1.1 500") && !r.contains("set-cookie"), "headers can't be injected: {r}");
        let r = http(port, "HEAD /json HTTP/1.1\r\n\r\n");
        assert!(r.contains("content-length: 5") && r.ends_with("\r\n\r\n"), "HEAD has no body: {r}");
        assert!(http(port, "TRACE / HTTP/1.1\r\n\r\n").starts_with("HTTP/1.1 501"));
    });
    let _ = server.kill();
    let _ = server.wait();
    if let Err(e) = results {
        std::panic::resume_unwind(e);
    }
    // `handle`'s signature is checked before serving
    save(&d.join("wrong.strata"), "fn handle(n: Int) -> Int {\n  n\n}\n");
    let (ok, out) = strata(&["serve", "wrong.strata"], &d, None);
    assert!(!ok && out.contains("Its parameter `n` has type `Int`"), "{out}");
}

#[test]
fn own_methods_locals_and_function_values() {
    let d = tmp("own-locals-fns");
    std::fs::write(d.join("strata.toml"), "name = \"m\"\n").unwrap();
    save_module(&d, "counter",
        "pub type Counter = { n: Natural }

impl Counter {
  fn bump(mut self) {
    self.n = self.n + 1
  }
}
");
    let src = "use counter.Counter

type Twice = { f: (Natural) -> Natural }

/// A function named like `Counter`'s method (in another module): `k.bump()`
/// is still the method.
fn bump(c: Counter) -> Natural {
  mut k := c
  k.bump()
  k.n
}

/// A local named like the function hides it: this is not recursion.
fn fresh() -> Counter {
  mut fresh := Counter { n: 0 }
  fresh.bump()
  fresh
}

/// What a function value returns is a valid value of its result type.
fn twice(t: Twice, x: Natural) -> Natural {
  f := t.f
  f(f(x))
}

/// A lambda passed for a function parameter gets valid arguments.
fn use_twice() -> Natural {
  twice(Twice { f: n -> n + 1 }, 3)
}

test \"methods\" {
  assert bump(Counter { n: 1 }) == 2
  assert fresh().n == 1
}
";
    save_module(&d, "main", src);
    let (ok, out) = strata(&["--file", ".", "test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");
    if !has_solver() {
        eprintln!("z3 is not installed: skipping");
        return;
    }
    let (_, out) = strata(&["--file", ".", "verify", "--checks"], &d, None);
    assert!(out.contains("✓ fresh") && !out.contains("may not terminate"), "a local doesn't call the function it is named like: {out}");
    assert!(out.contains("✓ twice"), "a function value's result is valid: {out}");
}

#[test]
fn serve_runs_an_app() {
    let d = tmp("serve-app");
    let port = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
    save(&d.join("app.strata"),
        r#"use std.web.{Html, div, button, text, on_click}
type Model = { n: Int where 0 <= it && it <= 3 }
type Msg =
  | Up
  | Set { n: Int where 0 <= it && it <= 3 }
  | Boom
fn init() -> Model {
  Model { n: 0 }
}
fn update(model: Model, msg: Msg) -> Model {
  match msg {
    Up => Model { n: min(model.n + 1, 3) },
    Set { n } => Model { n },
    Boom => Model { n: 10 / (model.n - model.n) },
  }
}
fn view(model: Model) -> Html[Msg] {
  div([], [text(f"n={model.n}"), button([on_click(Up)], [text("up")])])
}
"#);
    let mut server = Command::new(env!("CARGO_BIN_EXE_strata")).args(["serve", "app.strata", "--port", &port.to_string()]).current_dir(&d).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let up = (0..100).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
    });
    let post = |body: &str| http(port, &format!("POST /_strata/msg HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}", body.len()));
    let results = std::panic::catch_unwind(|| {
        assert!(up, "the server didn't start");
        let page = http(port, "GET / HTTP/1.1\r\n\r\n");
        assert!(page.contains("<title>app</title>") && page.contains(r#"<script src="/_strata/app.js""#) && page.contains("n=0"), "{page}");
        assert!(page.contains(r#"data-on-click="&quot;Up&quot;""#), "{page}");
        let sid = page.split(r#"data-session=""#).nth(1).unwrap().split('"').next().unwrap().to_string();
        // each page load is its own session
        let other = http(port, "GET / HTTP/1.1\r\n\r\n");
        assert!(!other.contains(&sid));
        let r = post(&format!(r#"{{"session":"{sid}","msg":"Up"}}"#));
        assert!(r.starts_with("HTTP/1.1 200") && r.ends_with(r#"<div>n=1<button data-on-click="&quot;Up&quot;">up</button></div>"#), "{r}");
        let r = post(&format!(r#"{{"session":"{sid}","msg":{{"Set":{{"n":3}}}}}}"#));
        assert!(r.contains("n=3"), "{r}");
        // a message is read against the type: refinements included
        let r = post(&format!(r#"{{"session":"{sid}","msg":{{"Set":{{"n":9}}}}}}"#));
        assert!(r.starts_with("HTTP/1.1 400") && r.contains("does not satisfy"), "{r}");
        // a fault in `update` is a 500, and the model stays as it was
        let r = post(&format!(r#"{{"session":"{sid}","msg":"Boom"}}"#));
        assert!(r.starts_with("HTTP/1.1 500") && r.contains("division by zero"), "{r}");
        assert!(post(&format!(r#"{{"session":"{sid}","msg":"Up"}}"#)).contains("n=3"));
        // an unknown session: the page reloads
        assert!(post(r#"{"session":"gone","msg":"Up"}"#).starts_with("HTTP/1.1 410"));
        assert!(http(port, "GET /_strata/app.js HTTP/1.1\r\n\r\n").contains("data-on-"));
    });
    let _ = server.kill();
    let _ = server.wait();
    if let Err(e) = results {
        std::panic::resume_unwind(e);
    }
    // the three functions' signatures are checked before serving
    save(&d.join("bad.strata"), "use std.web.{Html, text}\nfn init() -> Int {\n  0\n}\nfn update(m: Int, msg: String) -> String {\n  msg\n}\nfn view(m: Int) -> Html[String] {\n  text(\"x\")\n}\n");
    let (ok, out) = strata(&["serve", "bad.strata"], &d, None);
    assert!(!ok && out.contains("`update` takes and returns the model `Int`"), "{out}");
    // the example app is proven: `update` keeps the model's refinements, and
    // the view needs nothing more (`Msg` is valid by construction)
    let ex = format!("{}/examples/counter", env!("CARGO_MANIFEST_DIR"));
    let (ok, out) = strata(&["--file", &ex, "test"], &d, None);
    assert!(ok, "{out}");
    if has_solver() {
        let (ok, out) = strata(&["--file", &ex, "verify", "--force", "--require", "proven"], &d, None);
        assert!(ok && out.contains("✓ update") && out.contains("✓ view"), "{out}");
    }
}

#[test]
fn serve_apps_take_values_and_forms() {
    let d = tmp("serve-forms");
    let port = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
    let ex = format!("{}/examples/todos", env!("CARGO_MANIFEST_DIR"));
    let mut server = Command::new(env!("CARGO_BIN_EXE_strata")).args(["serve", &ex, "--port", &port.to_string()]).current_dir(&d).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let up = (0..100).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
    });
    let post = |body: &str| http(port, &format!("POST /_strata/msg HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}", body.len()));
    let results = std::panic::catch_unwind(|| {
        assert!(up, "the server didn't start");
        let page = http(port, "GET / HTTP/1.1\r\n\r\n");
        // handlers are numbered in page order: the form, then the title field
        assert!(page.contains(r#"<form class="new" data-handle-submit="0">"#) && page.contains(r#"data-handle-input="1""#), "{page}");
        let sid = page.split(r#"data-session=""#).nth(1).unwrap().split('"').next().unwrap().to_string();
        // what is typed comes back in the view
        let r = post(&format!(r#"{{"session":"{sid}","handler":1,"value":"Call Ada"}}"#));
        assert!(r.contains(r#"value="Call Ada""#) && r.contains("52 characters left"), "{r}");
        // a form whose title is empty: refused, said next to the field
        let r = post(&format!(r#"{{"session":"{sid}","handler":0,"fields":{{"title":"  ","priority":"high"}}}}"#));
        assert!(r.contains("Write something first.") && !r.contains("<span class=\"title\">  </span>"), "{r}");
        let r = post(&format!(r#"{{"session":"{sid}","handler":0,"fields":{{"title":"Call Ada","priority":"high"}}}}"#));
        assert!(r.contains(r#"<span class="title">Call Ada</span>"#) && r.contains(r#"value="""#), "{r}");
        // a handler the page doesn't have
        assert!(post(&format!(r#"{{"session":"{sid}","handler":99,"value":""}}"#)).starts_with("HTTP/1.1 400"));
        assert!(post(&format!(r#"{{"session":"{sid}","handler":0,"fields":{{"title":3}}}}"#)).starts_with("HTTP/1.1 400"));
    });
    let _ = server.kill();
    let _ = server.wait();
    if let Err(e) = results {
        std::panic::resume_unwind(e);
    }
    if has_solver() {
        // `update` keeps every title within bounds and the list within 50
        let (ok, out) = strata(&["--file", &ex, "verify", "--force", "--require", "proven"], &d, None);
        assert!(ok && out.contains("✓ update") && out.contains("✓ new_todo_form"), "{out}");
    }
}

/// docs/learn-strata.md's first code block is a whole Strata program: it must
/// store, check, pass its tests, and be exactly what `render` prints.
#[test]
fn learn_strata_code_is_valid_and_canonical() {
    let d = tmp("learn");
    let doc = std::fs::read_to_string(format!("{}/docs/learn-strata.md", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let start = doc.find("```rust\n").expect("a ```rust block") + "```rust\n".len();
    let code = &doc[start..start + doc[start..].find("```\n").unwrap()];
    save(&d.join("learn.strata"), code);
    let (ok, out) = strata(&["--file", "learn.strata", "check"], &d, None);
    assert!(ok, "{out}");
    let (ok, out) = strata(&["--file", "learn.strata", "test"], &d, None);
    assert!(ok && out.contains("0 failed"), "{out}");
    let (_, rendered) = strata(&["--file", "learn.strata", "render"], &d, None);
    assert_eq!(rendered, code, "docs/learn-strata.md: the code is not as `render` prints it");
}

#[test]
fn ranges_slices_and_comprehensions() {
    let d = tmp("sugar");
    let src = format!("{}/tests/data/sugar.strata", env!("CARGO_MANIFEST_DIR"));
    let (ok, out) = strata(&["--file", &src, "test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");
    // written as they were: the text read back in prints the same, also through a bundle
    let (ok, written) = strata(&["--file", &src, "render"], &d, None);
    assert!(ok, "{written}");
    save(&d.join("again.strata"), &written);
    assert_eq!(text(&d.join("again.strata")), written);
    let (ok, out) = strata(&["--file", &src, "pack", "sugar.strata"], &d, None);
    assert!(ok, "{out}");
    let (ok, out) = strata(&["--file", "sugar.strata", "render"], &d, None);
    assert!(ok && out == written, "{out}");
    let (ok, out) = strata(&["--file", &src, "run", "first", "[]"], &d, None);
    assert!(!ok && out.contains("does not satisfy"), "{out}");
    save(&d.join("last.strata"), "fn last(xs: List[Int]) -> Int { xs[len(xs) - 1] }");
    let (ok, out) = strata(&["--file", "last.strata", "run", "last", "[]"], &d, None);
    assert!(!ok && out.contains("index -1 out of range (length 0)"), "{out}");
    for (code, err) in [
        ("fn f(xs: List[Int]) -> List[Int] { xs[1..=] }", "`..=` needs an end"),
    ] {
        // it doesn't parse, so it can't be stored
        let (ok, out) = strata(&["--file", "e.strata", "edit"], &d, Some(code));
        assert!(!ok && out.contains(err), "{code}: {out}");
    }
}

#[test]
fn coalesce() {
    let d = tmp("coalesce");
    let src = format!("{}/tests/data/coalesce.strata", env!("CARGO_MANIFEST_DIR"));
    let (ok, out) = strata(&["--file", &src, "test"], &d, None);
    assert!(ok && out.contains("2 passed"), "{out}");
    let (ok, out) = strata(&["--file", &src, "render"], &d, None);
    assert!(ok, "{out}");
    save(&d.join("again.strata"), &out);
    assert_eq!(text(&d.join("again.strata")), out);
    // a short `or { value }` is written `??`, a short `except { value }` `!!`
    save(&d.join("e.strata"), "fn f(s: String) -> Int { parse_int(s) or { 0 } }\nfn g(r: Result[Int, String]) -> Int { r except { 0 } }\n");
    let (ok, out) = strata(&["--file", "e.strata", "render"], &d, None);
    assert!(ok && out.contains("parse_int(s) ?? 0") && out.contains("r !! 0"), "{out}");
    // each on its own type only
    save(&d.join("w.strata"), "fn f(x: Option[Int]) -> Int { x except { 0 } }\nfn g(x: Result[Int, String]) -> Int { x ?? 0 }\n");
    let (ok, out) = strata(&["--file", "w.strata", "check"], &d, None);
    assert!(!ok && out.contains("in f: `except` / `!!` handle a Result") && out.contains("in g: `or` / `??` handle an Option"), "{out}");
}

#[test]
fn dict_and_set_literals() {
    let d = tmp("collections");
    let src = format!("{}/tests/data/collections.strata", env!("CARGO_MANIFEST_DIR"));
    let (ok, out) = strata(&["--file", &src, "test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");
    let (ok, written) = strata(&["--file", &src, "render"], &d, None);
    assert!(ok, "{written}");
    save(&d.join("again.strata"), &written);
    assert_eq!(text(&d.join("again.strata")), written);
    let (ok, out) = strata(&["--file", &src, "pack", "c.strata"], &d, None);
    assert!(ok, "{out}");
    let (ok, out) = strata(&["--file", "c.strata", "render"], &d, None);
    assert!(ok && out == written, "{out}");
}

#[test]
fn record_defaults() {
    let d = tmp("defaults");
    let src = format!("{}/tests/data/defaults.strata", env!("CARGO_MANIFEST_DIR"));
    let (ok, out) = strata(&["--file", &src, "test"], &d, None);
    assert!(ok && out.contains("1 passed"), "{out}");
    let (ok, out) = strata(&["--file", &src, "render"], &d, None);
    assert!(ok, "{out}");
    save(&d.join("again.strata"), &out);
    assert_eq!(text(&d.join("again.strata")), out);
    // an argument: a field left out takes its default
    save(&d.join("r.strata"), "type R = { n: Int = 1, s: String }\nfn f(r: R) -> Int { r.n }\n");
    let (ok, out) = strata(&["--file", "r.strata", "run", "f", r#"R { s: "x" }"#], &d, None);
    assert!(ok && out.trim() == "1", "{out}");
    // checked where the type is defined
    save(&d.join("bad.strata"), "type R = { n: Int where it > 0 = 0 }\ntype S = { s: String = 1 }\n");
    let (ok, out) = strata(&["--file", "bad.strata", "check"], &d, None);
    assert!(!ok && out.contains("the default of `n`, 0, does not satisfy") && out.contains("default of field `s`: expected `String`"), "{out}");
    // the command line: a field with a default is an option
    let hello = format!("{}/examples/hello", env!("CARGO_MANIFEST_DIR"));
    let (ok, out) = strata(&["run", &hello, "--", "Ada", "--times", "2"], &d, None);
    assert!(ok && out.matches("hello, Ada!").count() == 2, "{out}");
    let (ok, out) = strata(&["run", &hello, "--", "--help"], &d, None);
    assert!(ok && out.contains("--times <Int>") && out.contains("(default: 1)"), "{out}");
}

/// Derived traits and `comptime`: stored and printed exactly, and refused by
/// the checker until deriving is done (IDEAS.md).
#[test]
fn derive_syntax_round_trips() {
    let d = tmp("derive-syntax");
    let text = r#"/// Show a record as its field names.
pub derivable(Record) trait Named
  requires len(Self.fields) > 0 {
  /// The names of the fields.
  derived fn names() -> List[String] {
    comptime Self.fields.map(f -> f.name)
  }
  fn shout(self) -> String
}

comptime fn kind_of(comptime f: Field) -> String {
  match comptime f.type {
    _ => f.name,
  }
}

type Point = { x: Int, y: Int }

derive Named for Point {
  fn shout(self) -> String {
    "point"
  }
}

type Pair = { a: Int, b: Int }

derive Named for Pair

fn derive(derive: Int) -> Int {
  derive + 1
}
"#;
    // stored as written; it doesn't check (`Point` and `Pair` aren't records), as the edit says
    let (ok, out) = strata(&["--file", "d.strata", "edit"], &d, Some(text));
    assert!(!ok && out.contains("written, but") && out.contains("`Point` isn't one"), "{out}");
    let (_, rendered) = strata(&["--file", "d.strata", "render"], &d, None);
    assert_eq!(rendered, text);
    // `derive` stays an ordinary name: the function `derive` checks
    let (_, out) = strata(&["--file", "d.strata", "check"], &d, None);
    assert!(!out.contains("error in derive:"), "{out}");
}

/// A derived trait, specialized for a record: `comptime`, `make`,
/// `try_make`, functions with `comptime` parameters, `match comptime`.
#[test]
fn derived_traits_are_specialized() {
    let d = tmp("derive-expand");
    let text = r#"use std.typing.{Record, Field}

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
"#;
    let (ok, out) = strata(&["--file", "p.strata", "edit"], &d, Some(text));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "p.strata"], &d, None);
    assert!(ok, "{out}");
    assert_eq!(out, "[\"x\", \"label\", \"on\"]\nPoint { x: 0, label: \"\", on: false }\nOk(Point { x: 3, label: \"here\", on: true })\nErr(\"x: `three` is not a number\")\n");
    // what can't be derived says why, at the `derive`
    let bad = [
        ("trait Plain { fn p(self) -> Int }\ntype A = { x: Int }\nderive Plain for A\n", "`Plain` can't be derived: only a `derivable` trait can"),
        ("use std.typing.Record\nderivable(Record) trait Sized { derived fn n(self) -> Int { comptime len(Self.fields) } }\ntype S =\n  | One\n  | Two\nderive Sized for S\n", "`Sized` can only be derived for a `Record`, and `S` isn't one"),
        ("use std.typing.Record\nderivable(Record) trait Sized\n  requires len(Self.fields) > 1 { derived fn n(self) -> Int { comptime len(Self.fields) } }\ntype A = { x: Int }\nderive Sized for A\n", "`A` doesn't meet what `Sized` requires: len(Self.fields) > 1"),
    ];
    for (k, (src, want)) in bad.iter().enumerate() {
        let f = format!("bad{k}.strata");
        let (ok, out) = strata(&["--file", &f, "edit"], &d, Some(src));
        assert!(!ok && out.contains(want) && out.contains("written, but"), "{out}");
        let (ok, out) = strata(&["--file", &f, "check"], &d, None);
        assert!(!ok && out.contains(want), "{out}");
    }
}

/// A variant's name is unique in its module only: another module's type of
/// the same name is told apart by its module.
#[test]
fn variant_names_are_per_module() {
    let d = tmp("variant-names");
    std::fs::write(d.join("strata.toml"), "name = \"vn\"\n").unwrap();
    let (ok, out) = strata(&["--file", "./", "edit", "geo"], &d, Some("pub type Shape =\n  | Circle { r: Int }\n  | Square { s: Int }\n"));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["--file", "./", "edit", "main"], &d, Some("use geo.Shape\n\ntype Circle = { cx: Int }\n\nfn main() {\n  a := geo.Circle { r: 1 }\n  b := Circle { cx: 2 }\n  print(f\"{a} {b}\")\n}\n"));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "."], &d, None);
    assert!(ok && out.contains(r#"Circle { r: 1 } Circle { cx: 2 }"#), "{out}");
    // in one module, still an error
    let (ok, out) = strata(&["--file", "one.strata", "edit"], &d, Some("type Shape =\n  | Circle { r: Int }\n  | Square { s: Int }\n\ntype Circle = { cx: Int }\n"));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["--file", "one.strata", "check"], &d, None);
    assert!(!ok && out.contains("must be unique in its module"), "{out}");
}

// A method taking another `Self` can't be called on a trait value with
// what may be another implementation (help: take the trait instead); the
// receiver itself, or what its `Self`-returning methods make, is fine.
#[test]
fn self_arguments_on_trait_values() {
    let d = tmp("dyn-self");
    let src = "trait Shape {\n  fn size(self) -> Int\n  fn grown(self) -> Self\n  fn bigger(self, other: Self) -> Bool\n  fn bigger_shape(self, other: Shape) -> Bool {\n    self.size() > other.size()\n  }\n}\n\ntype Sq = { side: Int }\n\ntype Ci = { r: Int }\n\nimpl Shape for Sq {\n  fn size(self) -> Int { self.side }\n  fn grown(self) -> Sq { Sq { side: self.side + 1 } }\n  fn bigger(self, other: Sq) -> Bool { self.side > other.side }\n}\n\nimpl Shape for Ci {\n  fn size(self) -> Int { self.r }\n  fn grown(self) -> Ci { Ci { r: self.r + 1 } }\n  fn bigger(self, other: Ci) -> Bool { self.r > other.r }\n}\n\nfn main() {\n  let xs: List[Shape] = [Sq { side: 2 }, Ci { r: 3 }]\n  for s in xs {\n    print(s.bigger(s.grown().grown()))\n    print(s.bigger_shape(xs[1]))\n  }\n  print(Sq { side: 1 }.bigger(Sq { side: 2 }))\n  MIXED\n}\n";
    let (ok, out) = strata(&["--file", "ok.strata", "edit"], &d, Some(&src.replace("MIXED", "")));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["--file", "ok.strata", "run", "--interpret"], &d, None);
    assert!(ok && out == "false\nfalse\nfalse\nfalse\nfalse\n", "{out}");
    let (ok, out) = strata(&["--file", "bad.strata", "edit"], &d, Some(&src.replace("MIXED", "print(xs[0].bigger(xs[1]))")));
    assert!(!ok && out.contains("written, but") && out.contains("`bigger` takes `other: Self`"), "{out}");
    let (ok, out) = strata(&["--file", "bad.strata", "check"], &d, None);
    assert!(!ok && out.contains("`bigger` takes `other: Self`, a value of the receiver's own type, but `xs[0]` is a `Shape`") && out.contains("help: to accept any `Shape`, declare the parameter so: `other: Shape`"), "{out}");

}

#[path = "common/json.rs"]
mod json_case;

/// `derive Json`: JSON as a web front end sends it, read into records and
/// sums with every refinement checked, and what has no JSON form refused
/// at the `derive`.
/// A union's members declared in it or defined elsewhere: a bare name is
/// the type of that name when there is one (`Circle`), else an atom it
/// declares (`Dot`); a union of unions flattens.
#[test]
fn unions_declare_members_or_name_them() {
    let d = tmp("unions-mixed");
    let (ok, out) = strata(&["--file", "u.strata", "edit"], &d, Some(UNIONS_PROGRAM));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "u.strata"], &d, None);
    assert!(ok, "{out}");
    assert_eq!(out, "3\n4\n0\ndown 3\nshape\n[Quiet, Loud]\n");
    // `Circle` (no doc, one union) is shown inside it; on its own with `--members separate`
    let (ok, text) = strata(&["--file", "u.strata", "render"], &d, None);
    assert!(ok && text.contains("type Shape =\n  | Circle { r: Int }\n  | Square { side: Int }\n  | Dot\n"), "{text}");
    let (ok, text) = strata(&["--file", "u.strata", "render", "--members", "separate"], &d, None);
    assert!(ok && text.contains("type Circle = { r: Int }\n") && text.contains("type Shape = Circle | Square | Dot\n"), "{text}");
}

const UNIONS_PROGRAM: &str = r##"type Circle = { r: Int }

type Shape = Circle | Square { side: Int } | Dot

type Level = Quiet | Loud

type Old =
  | Up
  | Down(Int)

type Both = Shape | Old

fn area(s: Shape) -> Int {
  match s {
    Circle => 3 * s.r * s.r,
    Square => s.side * s.side,
    Dot => 0,
  }
}

fn name(b: Both) -> String {
  match b {
    Circle | Square | Dot => "shape",
    Up => "up",
    Down(n) => f"down {n}",
  }
}

fn main() {
  print(area(Circle { r: 1 }))
  print(area(Square { side: 2 }))
  print(area(Dot))
  print(name(Down(3)))
  print(name(Dot))
  print([Quiet, Loud])
}
"##;

/// A named union's `impl`s and methods: for a value typed by the union (or
/// by the same set written out), the union's; a member's own for a member.
#[test]
fn named_unions_have_impls() {
    let d = tmp("unions-impls");
    let (ok, out) = strata(&["--file", "u.strata", "edit"], &d, Some(UNION_IMPLS));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "u.strata"], &d, None);
    assert!(ok, "{out}");
    assert_eq!(out, "circle 1\n3\ncircle 1\nsquare 2\n4\nsquare 2\ndot\n0\ndot\na circle on its own\ncircle 2\n");
}

const UNION_IMPLS: &str = r##"trait Show {
  fn show(self) -> String
}

type Circle = { r: Int }

type Shape = Circle | Square { side: Int } | Dot

impl Show for Shape {
  fn show(self) -> String {
    match self {
      Circle => f"circle {self.r}",
      Square => f"square {self.side}",
      Dot => "dot",
    }
  }
}

impl Show for Circle {
  fn show(self) -> String = "a circle on its own"
}

impl Shape {
  fn area(self) -> Int {
    match self {
      Circle => 3 * self.r * self.r,
      Square => self.side * self.side,
      Dot => 0,
    }
  }
}

fn describe[T: Show](x: T) -> String = x.show()

fn shapes() -> List[Shape] = [Circle { r: 1 }, Square { side: 2 }, Dot]

fn written(x: Circle | Square | Dot) -> String = x.show()

fn main() {
  for s in shapes() {
    print(s.show())
    print(s.area())
    print(describe(s))
  }
  print(Circle { r: 5 }.show())
  print(written(Circle { r: 2 }))
}
"##;

/// A named union that adds nothing is refused as a bare alias is: one type
/// once normalized, or a set its module (or the prelude) names already; a
/// `distinct` one is a type of its own. A new member named like an existing
/// type is a warning.
#[test]
fn unions_that_add_nothing_are_refused() {
    let d = tmp("unions-nothing");
    let cases = [
        ("type U = Int | Positive", Some("`type U = Int | Positive` adds nothing: it is `Int`")),
        ("type Circle = { r: Int }\ntype Square = { s: Int }\ntype Shape = Circle | Square\ntype Figure = Square | Circle", Some("`type Figure = Square | Circle` adds nothing: it is `Shape`")),
        ("type Answer = Some(Int) | None", Some("adds nothing: it is `Option`")),
        ("type Level = Quiet | Loud\ntype Mood = Loud | Quiet", Some("`type Mood = Loud | Quiet` adds nothing: it is `Level`")),
        ("type Shape = Circle | Square { s: Int } | Dot\ntype Circle = { r: Int }\ntype Part = Square | Dot", None),
        ("type Circle = { r: Int }\ntype Square = { s: Int }\ntype Shape = Circle | Square\ndistinct type Figure = Circle | Square\nfn f() -> Figure = Figure(Circle { r: 1 })", None),
        ("distinct type Lv = Up | Down", Some("`Lv` declares its members, so it is a type of its own already")),
        // a type parameter is wrapped in a member the union declares, as `Option`'s `Some(T)`
        ("type Problem = { why: String }\ntype OrError[T] = T | Problem", Some("`T` can't be a member of `OrError` on its own: wrap it")),
        ("type Two[A, B] = List[A] | List[B]", Some("`List[A]` can't be a member of `Two` next to another `List`")),
        ("type Many[T] = List[T] | Int", None),
        ("type Many[T] = List[T] | Int\ntype Ints = List[Int] | Int", Some("`type Ints = List[Int] | Int` adds nothing: it is `Many`")),
        ("type Problem = { why: String }\ntype OrError[T] = Value(T) | Problem", None),
    ];
    for (k, (src, want)) in cases.iter().enumerate() {
        let f = format!("n{k}.strata");
        // (written, then checked: the edit says what doesn't)
        let (ok, out) = strata(&["--file", &f, "edit"], &d, Some(&format!("{src}\n\nfn main() {{ }}\n")));
        match want {
            Some(w) => assert!(!ok && out.contains(w), "{src}: {out}"),
            None => assert!(ok, "{src}: {out}"),
        }
    }
    let (ok, out) = strata(&["--file", "w.strata", "edit"], &d, Some("type Circle = { r: Int }\ntype Shape = Circel | Dot\n"));
    assert!(ok && out.contains("warning: `Circel` in `Shape` is a new type (nothing has that name): did you mean `Circle`?") && !out.contains("`Dot`"), "{out}");
}

/// A union written out renders as its name when its module names that
/// set (and named subsets as their names); never as a `distinct` type.
#[test]
fn unions_render_by_their_names() {
    let d = tmp("unions-render");
    let (ok, out) = strata(&["--file", "r.strata", "edit"], &d, Some(RENDER_NAMES));
    assert!(ok, "{out}");
    let (ok, text) = strata(&["--file", "r.strata", "render"], &d, None);
    assert!(ok, "{text}");
    for want in ["type S2 = A | D\n", "fn f(x: A) -> Int", "fn g(x: S2) -> Int", "fn h(e: Problem) -> Int", "fn k(w: B | D) -> Int"] {
        assert!(text.contains(want), "{want}: {text}");
    }
    // and it reads back as the same program
    let (ok, out) = strata(&["--file", "r2.strata", "edit"], &d, Some(&text));
    assert!(ok, "{out}");
    let (ok, again) = strata(&["--file", "r2.strata", "render"], &d, None);
    assert!(ok && again == text, "{again}");
}

const RENDER_NAMES: &str = r##"type B = { x: Int }

type C = { y: Int }

type D = { z: Int }

type A = B | C

type Problem =
  | Empty
  | TooBig { max: Int }

distinct type Wrapped = B | D

type S2 = B | C | D

fn f(x: B | C) -> Int = 1

fn g(x: B | C | D) -> Int = 2

fn h(e: Empty | TooBig) -> Int = 3

fn k(w: B | D) -> Int = 4

fn main() { }
"##;

/// JSON for named unions: members declared in them or defined elsewhere,
/// tagged by `"type"` (a tuple-shaped one's element as `"value"`), and
/// members told apart by their JSON (`Int | String`).
#[test]
fn json_for_named_unions() {
    let d = tmp("json-unions");
    let (ok, out) = strata(&["--file", "j.strata", "edit"], &d, Some(JSON_UNIONS));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "j.strata"], &d, None);
    assert!(ok, "{out}");
    assert_eq!(out, "{\"type\":\"Doc\",\"shapes\":[{\"type\":\"Circle\",\"r\":1.0},{\"type\":\"Square\",\"side\":2.0},{\"type\":\"Moved\",\"value\":3},{\"type\":\"Dot\"}],\"ids\":[1,\"a\"]}\ntrue\nErr(\"shapes[1]: `Nope` is not one of Circle, Square, Moved, Dot\")\n");
}

const JSON_UNIONS: &str = r##"use std.json.{Json, parse, stringify}

type Circle = { r: Float }

derive Json for Circle

type Shape = Circle | Square { side: Float } | Moved(Int) | Dot

derive Json for Shape

type Id = Int | String

derive Json for Id

type Doc = { shapes: List[Shape], ids: List[Id] }

derive Json for Doc

fn main() {
  d := Doc { shapes: [Circle { r: 1.0 }, Square { side: 2.0 }, Moved(3), Dot], ids: [1, "a"] }
  match d.encode() {
    Ok(j) => {
      print(stringify(j))
      print(Doc.decode(j) == Ok(d))
    },
    Err(e) => print(e),
  }
  match parse("{\"shapes\": [\"Dot\", {\"type\": \"Nope\"}], \"ids\": []}") {
    Ok(j) => print(Doc.decode(j)),
    Err(e) => print(e),
  }
}
"##;

/// An atom or a tuple-shaped type on its own (`type Dot`, `type
/// Moved(Int)`), in several unions; and the two views of a union's members
/// (`render --members separate | inline`), which read back as the same
/// program.
#[test]
fn union_members_on_their_own_or_inline() {
    let d = tmp("unions-views");
    let (ok, out) = strata(&["--file", "s.strata", "edit"], &d, Some(STANDALONE));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "s.strata"], &d, None);
    assert!(ok && out == "dot\nmoved 2\nsquare 3\n5\n", "{out}");
    let (ok, inline) = strata(&["--file", "s.strata", "render", "--members", "inline"], &d, None);
    assert!(ok && inline.contains("type Shape =\n  | Square { side: Int }\n  | Mark\n") && inline.contains("type Mark =\n  | Dot\n  | Moved(Int)\n"), "{inline}");
    let (ok, out) = strata(&["--file", "i.strata", "edit"], &d, Some(&inline));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "i.strata"], &d, None);
    assert!(ok && out == "dot\nmoved 2\nsquare 3\n5\n", "{out}");
    let (ok, sep) = strata(&["--file", "i.strata", "render", "--members", "separate"], &d, None);
    assert!(ok && sep.contains("type Square = { side: Int }\n") && sep.contains("type Dot\n") && sep.contains("type Moved(Int)\n") && sep.contains("type Shape = Square | Mark\n"), "{sep}");
    let (ok, out) = strata(&["--file", "p.strata", "edit"], &d, Some(&sep));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "p.strata"], &d, None);
    assert!(ok && out == "dot\nmoved 2\nsquare 3\n5\n", "{out}");
}

const STANDALONE: &str = r##"type Dot

type Moved(Int)

type Square = { side: Int }

type Shape = Square | Dot | Moved

type Mark = Dot | Moved

fn f(s: Shape) -> String {
  match s {
    Square => f"square {s.side}",
    Dot => "dot",
    Moved(n) => f"moved {n}",
  }
}

fn g(m: Mark) -> Int = match m { Dot => 0, Moved(n) => n }

fn main() {
  print(f(Dot))
  print(f(Moved(2)))
  print(f(Square { side: 3 }))
  print(g(Moved(5)))
}
"##;

/// A generic union (its type parameters only in members it declares) has
/// `impl`s of its own too.
#[test]
fn generic_unions_have_impls() {
    let d = tmp("unions-generic");
    let (ok, out) = strata(&["--file", "g.strata", "edit"], &d, Some(GENERIC_UNION));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "g.strata"], &d, None);
    assert!(ok && out == "a value\nproblem: not positive\n5\n", "{out}");
}

const GENERIC_UNION: &str = r##"trait Show {
  fn show(self) -> String
}

type Problem = { why: String }

type OrError[T] = Value(T) | Problem

impl[T] Show for OrError[T] {
  fn show(self) -> String {
    match self {
      Value(_) => "a value",
      Problem => f"problem: {self.why}",
    }
  }
}

fn get(n: Int) -> OrError[Int] {
  if n > 0 {
    Value(n)
  } else {
    Problem { why: "not positive" }
  }
}

fn main() {
  print(get(3).show())
  print(get(0).show())
  match get(5) {
    Value(n) => print(n),
    Problem => print("no"),
  }
}
"##;

/// A generic union with a wrapped parameter (`List[T]`): its `impl`s, a
/// bare `List =>` arm, and `List[Int] | Problem` rendered as `Many[Int]`.
#[test]
fn generic_unions_of_wrapped_parameters() {
    let d = tmp("unions-wrapped");
    let (ok, out) = strata(&["--file", "g.strata", "edit"], &d, Some(WRAPPED_UNION));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "g.strata"], &d, None);
    assert!(ok && out == "3 items\nproblem: none\n", "{out}");
    let (ok, out) = strata(&["--file", "r.strata", "edit"], &d, Some("type Problem = { why: String }\n\ntype Many[T] = List[T] | Problem\n\nfn f(x: List[Int] | Problem) -> Int {\n  match x {\n    List => len(x),\n    Problem => 0,\n  }\n}\n\nfn main() {\n  print(f([1, 2]))\n}\n"));
    assert!(ok, "{out}");
    let (ok, text) = strata(&["--file", "r.strata", "render"], &d, None);
    assert!(ok && text.contains("fn f(x: Many[Int]) -> Int"), "{text}");
    let (ok, out) = strata(&["run", "r.strata"], &d, None);
    assert!(ok && out == "2\n", "{out}");
}

/// Two modules may name the same set, but not both implement a trait for it.
#[test]
fn unions_naming_the_same_set_in_two_modules() {
    let d = tmp("unions-two-modules");
    let edit = |m: &str, text: &str| strata(&["--file", "./", "edit", m], &d, Some(text));
    assert!(edit("shapes", "pub trait Show {\n  fn show(self) -> String\n}\n\npub type Circle = { r: Int }\n\npub type Square = { s: Int }\n").0);
    assert!(edit("geo", "use shapes.{Show, Circle, Square}\n\npub type Shape = Circle | Square\n\nimpl Show for Shape {\n  fn show(self) -> String = \"geo\"\n}\n").0);
    let (ok, out) = edit("ui", "use shapes.{Circle, Square}\n\npub type Figure = Circle | Square\n");
    assert!(ok, "{out}");
    let (ok, out) = edit("ui", "use shapes.{Show, Circle, Square}\n\npub type Figure = Circle | Square\n\nimpl Show for Figure {\n  fn show(self) -> String = \"ui\"\n}\n");
    assert!(!ok && out.contains("`ui.Figure` and `geo.Shape` name the same set and both implement `Show`"), "{out}");
}

const WRAPPED_UNION: &str = r##"trait Show {
  fn show(self) -> String
}

type Problem = { why: String }

type Many[T] = List[T] | Problem

impl[T] Show for Many[T] {
  fn show(self) -> String {
    match self {
      List => f"{len(self)} items",
      Problem => f"problem: {self.why}",
    }
  }
}

fn get(n: Int) -> Many[Int] {
  if n > 0 {
    [1, 2, n]
  } else {
    Problem { why: "none" }
  }
}

fn main() {
  print(get(3).show())
  print(get(0).show())
}
"##;

/// The shapes `std.typing` describes, through `Json`: a recursive type, a
/// generic one, a union of records, a tuple-shaped variant, `Option` of a
/// refined alias, a `distinct` type; and a union's members keep their docs.
#[test]
fn derives_see_every_kind_of_type() {
    let d = tmp("derive-kinds");
    let (ok, out) = strata(&["--file", "k.strata", "edit"], &d, Some(KINDS_PROGRAM));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "k.strata"], &d, None);
    assert!(ok, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], r#"{"type":"All","tree":{"type":"Node","left":{"type":"Leaf"},"value":1,"right":{"type":"Node","left":{"type":"Leaf"},"value":2,"right":{"type":"Leaf"}}},"pair":{"type":"Pair","a":1,"b":2},"events":[{"type":"Moved","value":3},{"type":"Drawn","shape":{"type":"Square","side":2.0},"at":80},{"type":"Drawn","shape":{"type":"Circle","r":1.0},"at":null}],"who":"u1"}"#);
    assert_eq!(lines[1], "true");
    assert_eq!(lines[2], r#"Err("events[0].at: 70000 does not satisfy `Port` (where 0 < it && it < 65536)")"#);
    assert_eq!(lines[3], r#"["Circle: A circle, by its radius.; Square: A square, by its side.", ""]"#);
}

const KINDS_PROGRAM: &str = r##"use std.json.Json

use std.typing.{Record, Union}

/// A circle, by its radius.
type Circle = { r: Float }

/// A square, by its side.
type Square = { side: Float }

/// One of the two.
type Shape = Circle | Square

type Port = Int
  where 0 < it && it < 65536

distinct type UserId = String

type Tree =
  | Leaf
  | Node { left: Tree, value: Int, right: Tree }

type Pair[T] = { a: T, b: T }

type Event =
  | Moved(Int)
  | Drawn { shape: Shape, at: Option[Port] }

type All = { tree: Tree, pair: Pair[Int], events: List[Event], who: UserId }

derive Json for Circle

derive Json for Square

derive Json for Shape

derive Json for Tree

derive Json for Pair

derive Json for Event

derive Json for All

/// The docs of a union field's members.
derivable(Record) trait Docs {
  derived fn docs(self) -> List[String] {
    comptime Self.fields.map(
      f -> join(f.type.members.map(
        m -> (m.name ?? "") + ": " + join(m.doc, " "),
      ), "; "),
    )
  }
}

type Holder = { s: Shape, n: Int }

derive Docs for Holder

fn main() {
  x := All { tree: Node {
    left: Leaf,
    value: 1,
    right: Node { left: Leaf, value: 2, right: Leaf },
  }, pair: Pair { a: 1, b: 2 }, events: [
    Moved(3),
    Drawn { shape: Square { side: 2.0 }, at: Some(80) },
    Drawn { shape: Circle { r: 1.0 }, at: None },
  ], who: UserId("u1") }
  match x.encode() {
    Ok(j) => {
      print(std.json.stringify(j))
      print(All.decode(j) == Ok(x))
    },
    Err(e) => print(e),
  }
  bad := std.json.parse(
    "{\"tree\": \"Leaf\", \"pair\": {\"a\": 1, \"b\": 2}, \"events\": [{\"type\": \"Drawn\", \"shape\": {\"type\": \"Circle\", \"r\": 1.0}, \"at\": 70000}], \"who\": \"u\"}",
  )
  match bad {
    Ok(j) => print(All.decode(j)),
    Err(e) => print(e),
  }
  print(Holder { s: Circle { r: 1.0 }, n: 1 }.docs())
}
"##;

#[test]
fn json_is_derived() {
    let d = tmp("derive-json");
    let (ok, out) = strata(&["--file", "j.strata", "edit"], &d, Some(json_case::PROGRAM));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "j.strata"], &d, None);
    assert!(ok, "{out}");
    assert_eq!(out, json_case::OUTPUT);
    let bad = [
        ("type A = { type: String }", "a field named `type` can't be written to JSON"),
        ("type A = { x: Option[Option[Int]] }", "`None` and `Some(None)` would both be `null`"),
        ("type A = { f: (Int) -> Int }", "`(Int) -> Int` can't be JSON"),
        ("type A =\n  | P(Int, Int)", "`P` has several positional fields"),
        ("type B = { x: Int }\n\ntype A = { b: B }", "`B` doesn't implement `Json`: write `derive Json for B`"),
        ("type A = { x: Int | Float }", "`Int | Float` can't be JSON: two members are written as a JSON number"),
        ("type A = { x: List[Int] | List[String] }", "two members are written as a JSON array"),
        ("type L =\n  | Quiet\n  | Loud\n\nderive Json for L\n\ntype A = { x: String | L }", "`Quiet`, `Loud` would be read as a String"),
    ];
    for (k, (src, want)) in bad.iter().enumerate() {
        let f = format!("bad{k}.strata");
        let (ok, out) = strata(&["--file", &f, "edit"], &d, Some(&format!("use std.json.Json\n\n{src}\n\nderive Json for A\n")));
        assert!(!ok && out.contains(want), "{src}: {out}");
        let (ok, out) = strata(&["--file", &f, "check"], &d, None);
        assert!(!ok && out.contains(want), "{src}: {out}");
    }
    // an integer beyond what a JSON number holds exactly isn't written:
    // `encode` returns an error, and verify has nothing to report
    let big = "use std.json.Json\n\ntype A = { x: Int }\n\nderive Json for A\n\nfn main() {\n  print(A { x: 9007199254740992 }.encode())\n  print(A { x: 3 }.encode() is Ok)\n}\n";
    let (ok, out) = strata(&["--file", "big.strata", "edit"], &d, Some(big));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "big.strata"], &d, None);
    assert!(ok && out == "Err(\"x: 9007199254740992 is beyond ±(2^53 - 1), the integers a JSON number holds exactly\")\ntrue\n", "{out}");
    let (ok, out) = strata(&["--file", "big.strata", "verify", "--cases", "20", "--require", "proven"], &d, None);
    assert!(ok && out.contains("✓ std.json.encode_as.for.A."), "{out}");
    // `integers = "proven"`: each integer field must be shown to be a `SafeInt`
    let q = d.join("proven");
    std::fs::create_dir_all(&q).unwrap();
    std::fs::write(q.join("strata.toml"), "name = \"proven\"\n\n[json]\nintegers = \"proven\"\n").unwrap();
    let (ok, out) = strata(&["--file", "./", "edit", "main"], &q, Some("use std.json.{Json, SafeInt}\n\ntype A = { x: Int }\n\nderive Json for A\n\ntype B = { id: SafeInt, n: Int where 0 <= it && it < 100 }\n\nderive Json for B\n"));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["verify", "--cases", "20"], &q, None);
    assert!(!ok && out.contains("✗ std.json.encode_as.for.A.") && out.contains("does not satisfy `std.json.SafeInt`") && !out.contains("✗ std.json.encode_as.for.B."), "{out}");
    // a package can make `decode` refuse keys a record doesn't have
    let p = d.join("strict");
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(p.join("strata.toml"), "name = \"strict\"\n\n[json]\nunknown_keys = \"error\"\n").unwrap();
    let src = "use std.json.{Json, Ignore, parse}\n\ntype A = { x: Int }\n\nderive Json for A\n\nfn main() {\n  j := parse(\"{\\\"x\\\": 1, \\\"y\\\": 2}\") !! std.json.Null\n  print(A.decode(j))\n  print(A.decode_with(j, Ignore))\n}\n";
    let (ok, out) = strata(&["--file", "./", "edit", "main"], &p, Some(src));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "."], &p, None);
    assert!(ok && out == "Err(\"unknown field \\\"y\\\"\")\nOk(A { x: 1 })\n", "{out}");
    std::fs::write(p.join("strata.toml"), "name = \"strict\"\n\n[json]\nunknown_keys = \"sometimes\"\n").unwrap();
    let (ok, out) = strata(&["run", "."], &p, None);
    assert!(!ok && out.contains("unknown_keys is \"error\" or \"ignore\""), "{out}");
}

// `extern c`: foreign declarations, checked, kept in their module, never run
// by the interpreter or by `verify`.
#[test]
fn extern_c_declarations() {
    let d = tmp("extern_c");
    let p = d.join("p");
    std::fs::create_dir_all(&p).unwrap();
    let db = "/// An open database.\nextern c type Sqlite3\n\nextern c fn strlen(s: String) -> U64\n  ensures it >= 0\n\nextern c fn sqlite3_open(path: String, mut db: Sqlite3) -> I32\n\npub fn length(s: String) -> U64 {\n  strlen(s)\n}\n";
    save_module(&p, "db", db);
    save_module(&p, "main", "use db\n\nfn main() {\n  print(db.length(\"abc\"))\n}\n");
    // the text round-trips
    let (ok, out) = strata(&["--file", "p", "render", "db"], &d, None);
    assert!(ok && out == db, "{out}");
    let (ok, out) = strata(&["--file", "p", "check"], &d, None);
    assert!(ok, "{out}");
    // the interpreter refuses to call C
    let (ok, out) = strata(&["--file", "p", "run", "--interpret"], &d, None);
    assert!(!ok && out.contains("`strlen` is an `extern c` function: only compiled code calls it"), "{out}");
    // verify runs nothing that reaches C
    let (_, out) = strata(&["--file", "p", "verify"], &d, None);
    assert!(out.contains("db.length") && out.contains("assumed: module `db` calls foreign code"), "{out}");
    assert!(out.contains("assumed: it calls `db.length`, in module `db`, which calls foreign code"), "{out}");
    // only its own module calls it, submodules included
    save_module(&p, "db.inner", "fn sub() -> U64 { db.strlen(\"x\") }\n");
    let (ok, out) = strata(&["--file", "p", "check"], &d, None);
    assert!(!ok && out.contains("`db.strlen` is a foreign function: only module `db` can call it"), "{out}");
    std::fs::remove_dir_all(p.join("db")).unwrap();
    // what crosses to C, and never `pub`
    save_module(&p, "bad", "pub extern c fn abs(x: Int) -> List[Int]\n\nextern c fn f(mut s: String)\n\nextern c type H\n\nfn forge() -> H { H(1) }\n");
    let (ok, out) = strata(&["--file", "p", "check"], &d, None);
    assert!(!ok, "{out}");
    for m in [
        "`extern c` definitions are never `pub`",
        "`extern c fn abs`, parameter `x`: `Int` has no fixed size",
        "`extern c fn abs`, result: `List[Int]` doesn't cross to C",
        "`extern c fn f`, parameter `s`: a `String` can't be `mut`",
        "unknown constructor `bad.H`",
    ] {
        assert!(out.contains(m), "{m}: {out}");
    }
    std::fs::remove_file(p.join("bad.strata")).unwrap();
    // a body, or another language, doesn't parse
    let (ok, out) = strata(&["--file", "x.strata", "edit"], &d, Some("extern c fn f() -> I32 { 1 }\n"));
    assert!(!ok && out.contains("has no body: it is defined in C"), "{out}");
    let (ok, out) = strata(&["--file", "x.strata", "edit"], &d, Some("extern cobol fn f() -> I32\n"));
    assert!(!ok && out.contains("unknown foreign language `cobol`"), "{out}");
    // its name is the C symbol: no rename
    let pkg = d.join("pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    let (ok, out) = strata(&["init"], &pkg, None);
    assert!(ok, "{out}");
    let (ok, out) = strata(&["edit", "db", "--no-commit"], &pkg, Some("extern c fn strlen(s: String) -> U64\n\npub fn length(s: String) -> U64 { strlen(s) }\n"));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["rename", "db.strlen", "c_strlen", "--no-commit"], &pkg, None);
    assert!(!ok && out.contains("`db.strlen` is a foreign function: its name is the symbol it is linked to"), "{out}");
}

// Compile-time code that computes (not only data): a function of the
// program, a lambda, a big integer; a fault in it and a runaway loop are
// errors of the `derive`, showing the code as written.
#[test]
fn comptime_code_runs_while_compiling() {
    let d = tmp("comptime-runs");
    let src = "use std.typing.Record\n\nfn shout(s: String) -> String {\n  s + \"!\"\n}\n\nfn spin(n: Int) -> Int {\n  mut i := n\n  while i > 0 { i = i + 1 }\n  i\n}\n\nderivable(Record) trait Names\n  requires len(Self.fields) > 1 {\n  derived fn names(self) -> List[String] {\n    comptime Self.fields.map(f -> shout(f.name))\n  }\n  derived fn count(self) -> Int {\n    comptime COUNT\n  }\n}\n\ntype P = { x: Int, label: String }\n\nderive Names for P\n\nfn main() {\n  p := P { x: 1, label: \"a\" }\n  print(p.names())\n  print(p.count())\n}\n";
    let run = |count: &str| {
        // stored either way; the edit already says when it doesn't check
        let (ok, out) = strata(&["--file", "c.strata", "edit"], &d, Some(&src.replace("COUNT", count)));
        assert!(ok || out.contains("written, but"), "{out}");
        strata(&["--file", "c.strata", "run"], &d, None)
    };
    let (ok, out) = run("(99999999999999999999999 * len(Self.fields))");
    assert!(ok && out == "[\"x!\", \"label!\"]\n199999999999999999999998\n", "{out}");
    let (ok, out) = run("(len(Self.fields) / 0)");
    assert!(!ok && out.contains("deriving `count`: `comptime len(Self.fields) / 0` failed: division by zero"), "{out}");
    let (ok, out) = run("spin(1)");
    assert!(!ok && out.contains("`comptime spin(1)` ran for more than 5000000 steps: compile-time code must finish"), "{out}");
}

/// `comptime e` in ordinary code: evaluated while compiling, its value
/// written in (the program never calls what computed it); a variable around
/// it can't be used. Evaluating it once made the program resolve itself
/// again, without end (each level evaluating the same `comptime`), until
/// memory ran out: this would hang.
#[test]
fn comptime_in_ordinary_code_runs_while_compiling() {
    let d = tmp("comptime-anywhere");
    let src = "use std.regex.{parse, show}\n\nfn shout(s: String) -> String {\n  s + \"!\"\n}\n\nfn main() {\n  r := comptime parse(\"(?i)ab|ac\")\n  print(match r {\n    Ok(x) => show(x),\n    Err(e) => e,\n  })\n  print(comptime shout(\"hi\"))\n}\n";
    let (ok, out) = strata(&["--file", "c.strata", "edit"], &d, Some(src));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["--file", "c.strata", "run"], &d, None);
    assert!(ok && out == "cat(cls(41-41 61-61),cls(42-43 62-63))\nhi!\n", "{out}");
    let (ok, out) = strata(&["--file", "c.strata", "build", "--emit-llvm", "-o", "c.ll"], &d, None);
    let ir = std::fs::read_to_string(d.join("c.ll")).unwrap_or_default();
    assert!(ok && ir.contains("define") && !ir.contains("std.regex.parse") && !ir.contains("@\"strata:shout\""), "the values are constants: {out}{ir}");
    // the edit says so at once (it is stored, as a script's edit is)
    let (ok, out) = strata(&["--file", "v.strata", "edit"], &d, Some("fn main() {\n  n := 3\n  print(comptime n + 1)\n}\n"));
    assert!(!ok && out.contains("unknown variable `n`") && out.contains("written, but"), "{out}");
    let (ok, out2) = strata(&["--file", "v.strata", "check"], &d, None);
    assert!(!ok && out2.contains("unknown variable `n`: compile-time code can use definitions, not the variables around it"), "{out}{out2}");
}

/// `for` over a value implementing `Iterator`: run, refused when the value
/// isn't one, and not shown to end by `verify`.
#[test]
fn for_over_iterators() {
    let d = tmp("iterators");
    let (ok, out) = strata(&["--file", "it.strata", "edit"], &d, Some(json_case::ITERATORS));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "it.strata"], &d, None);
    assert!(ok, "{out}");
    assert_eq!(out, json_case::ITERATORS_OUTPUT);
    let (_, out) = strata(&["--file", "it.strata", "verify"], &d, None);
    assert!(out.contains("it has a `for` over an `Iterator` that isn't a `FiniteIterator`, which may never end"), "{out}");
    // the defaults of the prelude's traits are library code, not the program's
    assert!(!out.contains("Count.map") && !out.contains("Count.fold"), "{out}");
    assert!(out.contains("index -1 out of range (length 0) (stack: Items.next)"), "{out}");
    // `self.inner.next()` in `Padded.next` is recursion on a smaller value; the
    // fault it then reaches is `Items.next`'s, not the prover's
    assert!(!out.contains("recursive through") && !out.contains("bug in the prover"), "{out}");
    assert!(out.contains("It fails in `Items.next`, which it calls"), "{out}");
    let bad = "type P = { x: Int }\n\nfn a(p: P) {\n  for x in p {\n    print(x)\n  }\n}\n\nfn b(s: String) {\n  for c in s {\n    print(c)\n  }\n}\n\nfn c[I](it: I) {\n  for x in it {\n    print(x)\n  }\n}\n";
    // written, and reported as not checking
    let (_, out) = strata(&["--file", "bad.strata", "edit"], &d, Some(bad));
    assert!(out.contains("written, but bad.strata doesn't check"), "{out}");
    let (ok, out) = strata(&["--file", "bad.strata", "check"], &d, None);
    assert!(!ok, "{out}");
    assert!(out.contains("error in a: `for` needs a List, Set, Dict or an `Iterator`: `P` does not implement `Iterator`"), "{out}");
    assert!(out.contains("error in b: `for` needs a List, Set, Dict or an `Iterator`, found `String`"), "{out}");
    assert!(out.contains("error in c: `for` needs a List, Set, Dict or an `Iterator`: `I` is not known to implement `Iterator`"), "{out}");
}

/// `FiniteIterator` and lazy `map` / `filter` / `fold`: run, and a `for`
/// over a finite iterator shown to end, over an unbounded one not.
#[test]
fn finite_and_lazy_iterators() {
    let d = tmp("lazy-iterators");
    let (ok, out) = strata(&["--file", "lazy.strata", "edit"], &d, Some(json_case::LAZY_ITERATORS));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "lazy.strata"], &d, None);
    assert!(ok, "{out}");
    assert_eq!(out, json_case::LAZY_ITERATORS_OUTPUT);
    let (_, out) = strata(&["--file", "lazy.strata", "verify"], &d, None);
    let after = |f: &str| out.split(&format!("✓ {f} ")).nth(1).and_then(|r| r.lines().nth(1)).unwrap_or("").to_string();
    for f in ["total", "size"] {
        assert!(!after(f).contains("may not terminate"), "{f} ends: {out}");
    }
    assert!(after("first_square_over").contains("it has a `for` over an `Iterator` that isn't a `FiniteIterator`"), "{out}");
    assert!(!out.contains("Count.map") && !out.contains("Mapped"), "{out}");
}

/// `fold` on finite iterators only, shown to end with its lambdas; and
/// comprehensions over iterators.
#[test]
fn fold_ends_and_comprehensions_over_iterators() {
    let d = tmp("iterator-comps");
    let (ok, out) = strata(&["--file", "c.strata", "edit"], &d, Some(json_case::ITERATOR_COMPREHENSIONS));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "c.strata"], &d, None);
    assert!(ok, "{out}");
    assert_eq!(out, json_case::ITERATOR_COMPREHENSIONS_OUTPUT);
    let (_, out) = strata(&["--file", "c.strata", "verify"], &d, None);
    let after = |f: &str| out.split(&format!("✓ {f} ")).nth(1).and_then(|r| r.lines().nth(1)).unwrap_or("").to_string();
    for f in ["ok_sum", "filtered_len"] {
        assert!(!after(f).contains("may not terminate"), "{f} ends: {out}");
    }
    assert!(after("bad_lambda").contains("it calls `spin`, which may not terminate"), "{out}");
    // `fold` on an unbounded iterator: never returns, so refused
    let inf = "type N = { at: Int }\n\nimpl Iterator[Int] for N {\n  fn next(mut self) -> Option[Int] {\n    self.at += 1\n    Some(self.at)\n  }\n}\n\nfn f() -> Int {\n  N { at: 0 }.fold(0, (a, b) -> a + b)\n}\n\nfn g() -> Int {\n  N { at: 0 }.map(x -> x * 2).fold(0, (a, b) -> a + b)\n}\n";
    let (_, out) = strata(&["--file", "inf.strata", "edit"], &d, Some(inf));
    assert!(out.contains("error in f: `fold` needs a `FiniteIterator`: `N` is an `Iterator` but not a `FiniteIterator`"), "{out}");
    assert!(out.contains("error in g: calling `fold`: `Mapped[N, Int, Int]` implements `FiniteIterator` only when"), "{out}");
}

/// `iter()` on lists, sets and dicts: finite iterators (loops over them end);
/// the prelude's `ListIter` can't be defined again.
#[test]
fn collections_have_iterators() {
    let d = tmp("collection-iterators");
    let (ok, out) = strata(&["--file", "c.strata", "edit"], &d, Some(json_case::COLLECTION_ITERATORS));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "c.strata"], &d, None);
    assert!(ok, "{out}");
    assert_eq!(out, json_case::COLLECTION_ITERATORS_OUTPUT);
    let (_, out) = strata(&["--file", "c.strata", "verify"], &d, None);
    assert!(!out.contains("may not terminate"), "{out}");
    let (_, out) = strata(&["--file", "d.strata", "edit"], &d, Some("type ListIter = { n: Int }\n"));
    assert!(out.contains("`ListIter` is already defined in the prelude"), "{out}");
}

/// A string's units: bytes, code points and graphemes; the graphemes agree
/// with every case of Unicode's GraphemeBreakTest.txt.
#[test]
fn strings_have_bytes_code_points_and_graphemes() {
    let d = tmp("string-units");
    let (ok, out) = strata(&["--file", "u.strata", "edit"], &d, Some(json_case::STRING_UNITS));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "u.strata"], &d, None);
    assert!(ok, "{out}");
    assert_eq!(out, json_case::STRING_UNITS_OUTPUT);
    let cases = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/grapheme_break_test.st")).unwrap();
    let (ok, out) = strata(&["--file", "g.strata", "edit"], &d, Some(&cases));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "g.strata"], &d, None);
    assert!(ok && out.ends_with("1093 cases, 0 wrong\n"), "{out}");
}

/// `len`, indexing and slicing on an iterator; an index out of range is the
/// precondition of `at`, as written.
#[test]
fn iterators_are_indexed_and_sliced() {
    let d = tmp("iterator-indexing");
    let (ok, out) = strata(&["--file", "i.strata", "edit"], &d, Some(json_case::ITERATOR_INDEXING));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "i.strata"], &d, None);
    assert!(ok, "{out}");
    assert_eq!(out, json_case::ITERATOR_INDEXING_OUTPUT);
    let (ok, out) = strata(&["--file", "o.strata", "edit"], &d, Some("fn main() {\n  print(\"ab\".graphemes()[5])\n}\n"));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["run", "o.strata"], &d, None);
    assert!(!ok && out.contains("precondition of `std.unicode.Graphemes.at` violated: requires 0 <= i && i < self.len()"), "{out}");
}

/// Types passed whole at runtime: `T.m()` on an impl's type parameters, and
/// `std.json` for the built-in types through `T: Json`.
#[test]
fn impl_type_parameters_at_runtime() {
    let d = tmp("impl-statics");
    for (name, prog, want) in [("statics", json_case::STATICS, json_case::STATICS_OUTPUT), ("builtins", json_case::BUILTINS, json_case::BUILTINS_OUTPUT), ("text", json_case::TEXT, json_case::TEXT_OUTPUT), ("self_methods", json_case::SELF_METHODS, json_case::SELF_METHODS_OUTPUT), ("generic_derive", json_case::GENERIC_DERIVE, json_case::GENERIC_DERIVE_OUTPUT), ("option_option", json_case::OPTION_OPTION, json_case::OPTION_OPTION_OUTPUT), ("root_builtin_name", json_case::ROOT_BUILTIN_NAME, json_case::ROOT_BUILTIN_NAME_OUTPUT), ("refined", json_case::REFINED, json_case::REFINED_OUTPUT), ("refined_more", json_case::REFINED_MORE, json_case::REFINED_MORE_OUTPUT)] {
        let f = format!("{name}.strata");
        let (ok, out) = strata(&["--file", &f, "edit"], &d, Some(prog));
        assert!(ok, "{out}");
        let (ok, out) = strata(&["run", &f], &d, None);
        assert!(ok, "{out}");
        assert_eq!(out, want);
    }
}

// The standard library's folder is edited as a package, checked: a module
// named `io` there is `std.io`, nothing is logged, and an edit that breaks
// it is refused, as `strata edit` in it or `--file std` (both stored it).
#[test]
fn std_is_edited_as_a_checked_package() {
    let d = tmp("std-edit");
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("std");
    let std = d.join("std");
    // std's files and submodules (`std/ast/binary.strata`), not the
    // `.strata` folder an edit of std leaves there
    fn copy(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            if e.path().is_dir() {
                if e.file_name() != ".strata" {
                    copy(&e.path(), &to.join(e.file_name()));
                }
            } else {
                std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
            }
        }
    }
    copy(&src, &std);
    let (ok, out) = strata(&["edit", "io"], &std, Some("pub fn shout(s: String) -> String {\n  s + \"!\"\n}\n"));
    assert!(ok && out.contains("std.io.shout"), "{out}");
    assert!(!std.join("strata.log").exists() && !std.join("io").exists());
    let (ok, out) = strata(&["--file", "std", "edit", "std.io"], &d, Some("pub fn smallest(xs: List[Int], min: Int) -> Int {\n  fold(xs, min, (a, b) -> min(a, b))\n}\n"));
    assert!(!ok && out.contains("`Int` is not a function") && out.contains("nothing was changed"), "{out}");
    let (ok, out) = strata(&["--file", "std", "check"], &d, None);
    assert!(ok, "{out}");
}

// Capabilities: made only by the runtime, for `main` and tests that ask;
// capability traits for everything else.
#[test]
fn capabilities_given_by_the_runtime() {
    let d = tmp("capabilities");
    let p = d.join("p");
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(p.join("strata.toml"), "name = \"p\"\n").unwrap();
    let clock = "/// Reading the time.\npub capability trait Clock {\n  fn now(self) -> Int\n}\n\n/// The machine's clock.\npub capability SystemClock {\n  /// Seconds, as the system tells.\n  fn seconds() -> Int {\n    42\n  }\n}\n\nimpl Clock for SystemClock {\n  fn now(self) -> Int {\n    self.seconds()\n  }\n}\n\npub type Fixed = { at: Int }\n\nimpl Clock for Fixed {\n  fn now(self) -> Int {\n    self.at\n  }\n}\n\npub fn late(c: Clock) -> Bool {\n  c.now() > 40\n}\n";
    save_module(&p, "clock", clock);
    let main = "use clock.{SystemClock, Fixed, late}\n\nfn main(sys: SystemClock) {\n  print(f\"{sys.seconds()} {late(sys)} {late(Fixed { at: 1 })}\")\n}\n\ntest \"the real clock\" (sys: SystemClock) {\n  assert late(sys)\n}\n\ntest \"a fixed clock\" {\n  assert !late(Fixed { at: 1 })\n}\n";
    save_module(&p, "main", main);
    // the text round-trips
    for (m, src) in [("clock", clock), ("main", main)] {
        let (ok, out) = strata(&["--file", "p", "render", m], &d, None);
        assert!(ok && out == src, "{out}");
    }
    let (ok, out) = strata(&["--file", "p", "check"], &d, None);
    assert!(ok, "{out}");
    // the runtime makes it: compiled, interpreted, built
    for args in [&["run", "p"][..], &["run", "--interpret", "p"][..]] {
        let (ok, out) = strata(args, &d, None);
        assert!(ok && out == "42 true false\n", "{args:?}: {out}");
    }
    let (ok, out) = strata(&["--file", "p", "test"], &d, None);
    assert!(ok && out.contains("✓ the real clock") && out.contains("2 passed"), "{out}");
    let (ok, out) = strata(&["--file", "p", "test", "--interpret"], &d, None);
    assert!(ok && out.contains("2 passed"), "{out}");
    let exe = d.join("clockprog");
    let (ok, out) = strata(&["build", "p", "-o", &exe.to_string_lossy()], &d, None);
    if ok {
        let o = Command::new(&exe).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&o.stdout), "42 true false\n");
    } else {
        assert!(out.contains("clang"), "{out}");
    }
    // verify runs nothing that needs one
    let (_, out) = strata(&["--file", "p", "verify"], &d, None);
    assert!(out.contains("assumed: `SystemClock` is a capability") && out.contains("assumed: it takes the capability `SystemClock`"), "{out}");
    assert!(out.contains("clock.late") && !out.contains("panicked"), "{out}");
    // nothing else makes one
    save_module(&p, "bad", "use clock.{SystemClock}\n\nfn forge() -> SystemClock {\n  SystemClock {}\n}\n\nfn mocked() -> SystemClock {\n  mock SystemClock\n}\n\ntest \"asks for an int\" (n: Int) {\n  assert n == 1\n}\n");
    let (ok, out) = strata(&["--file", "p", "check"], &d, None);
    assert!(!ok, "{out}");
    for m in [
        "`SystemClock` is a capability: only the runtime makes one, for `main` or a test that asks for it (`system_clock: SystemClock`)",
        "`mock` needs a trait, found `clock.SystemClock`",
        "test parameter `n: Int`: a test asks only for capabilities",
    ] {
        assert!(out.contains(m), "{m}: {out}");
    }
    std::fs::remove_file(p.join("bad.strata")).unwrap();
    // its functions don't take `self`
    let (ok, out) = strata(&["--file", "x.strata", "edit"], &d, Some("capability C {\n  fn f(self) -> Int {\n    1\n  }\n}\n"));
    assert!(!ok && out.contains("a capability's functions don't take `self`"), "{out}");
    // std's: `main` names `Disk` and `Process`, and calls their traits' methods
    // without the traits imported; `Files` itself is for code below `main`
    save(&d.join("std_caps.strata"), "use std.fs.{Disk}\nuse std.process.{Process}\n\nfn main(mut disk: Disk, process: Process) {\n  mut disk.write(\"cap.txt\", \"hi\") except { () }\n  print(disk.read(\"cap.txt\") except { \"?\" })\n  mut disk.remove(\"cap.txt\") except { () }\n  print(disk.exists(\"cap.txt\"))\n  print(len(process.args()))\n}\n");
    for args in [&["run", "std_caps.strata", "--", "a", "b"][..], &["run", "--interpret", "std_caps.strata", "--", "a", "b"][..]] {
        let (ok, out) = strata(args, &d, None);
        assert!(ok && out == "hi\nfalse\n2\n", "{args:?}: {out}");
    }
    save(&d.join("asks_files.strata"), "use std.fs.{Files}\n\nfn main(files: Files) {\n  print(files.exists(\"x\"))\n}\n");
    let (ok, out) = strata(&["run", "asks_files.strata"], &d, None);
    assert!(!ok && out.contains("Name the capability instead, `disk: Disk` (std.fs), and pass it where `Files` is wanted"), "{out}");
}

/// `verify` checks a generic impl's methods at a whole type whose type
/// arguments meet the impl's bounds (`List[String]` for `impl[T: Zero] ..
/// for List[T]` when only `String` is a `Zero`), not at its head alone.
#[test]
fn generic_impls_are_verified_at_a_whole_type() {
    let d = tmp("fuzz-whole-types");
    let src = "trait Zero {\n  fn zero() -> Self\n}\n\nimpl Zero for String {\n  fn zero() -> Self {\n    \"z\"\n  }\n}\n\nimpl[T: Zero] Zero for List[T] {\n  fn zero() -> Self {\n    [T.zero(), T.zero()]\n  }\n}\n";
    let (ok, out) = strata(&["--file", "z.strata", "edit"], &d, Some(src));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["--file", "z.strata", "verify", "--cases", "10"], &d, None);
    assert!(ok && out.contains("✓ List[T].zero") && !out.contains("✗"), "{out}");
}

// What the environment chooses: `Console` (the terminal; a test's own output
// in a test), and how the program is run (`RunMode`), hidden unless
// strata.toml says `mode = true` under `[run]`. `--trace` records the console's
// calls with the rest.
#[test]
fn run_mode_console_in_tests_and_trace() {
    let d = tmp("run_mode");
    let p = d.join("p");
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(p.join("strata.toml"), "name = \"p\"\n").unwrap();
    let src = "use std.io.{Console}\nuse std.process.{RunMode, Running, Testing, Tracing}\n\nfn describe(mode: RunMode) -> String {\n  match mode {\n    Running => \"running\",\n    Testing => \"testing\",\n    Tracing => \"tracing\",\n  }\n}\n\nfn main(mut console: Console, mode: RunMode) {\n  mut console.print(describe(mode))\n}\n\ntest \"sees it\" (mode: RunMode, mut console: Console) {\n  mut console.print(\"hidden: the test passes\")\n  assert mode == Testing\n}\n\ntest \"fails\" (mut console: Console) {\n  mut console.print(\"shown: the test fails\")\n  assert false\n}\n";
    save_module(&p, "main", src);
    // hidden by default
    let (ok, out) = strata(&["--file", "p", "check"], &d, None);
    assert!(!ok && out.contains("test parameter `mode: RunMode`: how the program is run is hidden by default; say `mode = true` under `[run]`"), "{out}");
    std::fs::write(p.join("strata.toml"), "name = \"p\"\n\n[run]\nmode = true\n").unwrap();
    for (args, want) in [(&["--file", "p", "run"][..], "running\n"), (&["--file", "p", "run", "--interpret"][..], "running\n")] {
        let (ok, out) = strata(args, &d, None);
        assert!(ok && out == want, "{args:?}: {out}");
    }
    // under --trace: `Tracing`, and the console's calls are in the trace
    let o = Command::new(env!("CARGO_BIN_EXE_strata")).args(["--file", "p", "run", "--trace"]).current_dir(&d).output().unwrap();
    let (stdout, stderr) = (String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert_eq!(stdout, "tracing\n");
    assert!(stderr.lines().any(|l| l.contains(r#""event":"call""#) && l.contains("std.io.Terminal") && l.contains(r#""tracing""#)), "{stderr}");
    // in a test: `Testing`, and the console's output is the test's, shown only if it fails
    let (ok, out) = strata(&["--file", "p", "test"], &d, None);
    assert!(!ok && out.contains("✓ sees it") && out.contains("✗ fails") && out.contains("shown: the test fails") && !out.contains("hidden: the test passes"), "{out}");
}

// Mocks: fakes for tests, declared as types; their implementations may leave
// methods out (generated); for tests only; hidden from views unless asked.
#[test]
fn mocks_for_tests() {
    let d = tmp("mocks");
    let src = "use std.fs.{Files, FsError}\n\n/// An in-memory disk, for tests.\nmock MemDisk = { files: Dict[String, String] = Dict[String, String] {} }\n\nimpl Files for MemDisk {\n  fn read(self, path: String) -> Result[String, FsError] {\n    Ok(get(self.files, path) ?? \"nothing\")\n  }\n\n  fn write(mut self, path: String, text: String) -> Result[(), FsError] {\n    self.files = insert(self.files, path, text)\n    Ok(())\n  }\n}\n\nfn save_then_load(mut fs: Files, text: String) -> Result[String, FsError] {\n  (mut fs.write(\"a.txt\", text))!\n  fs.read(\"a.txt\")\n}\n\ntest \"saving then loading\" {\n  mut fs := MemDisk {}\n  assert save_then_load(mut fs, \"hi\") == Ok(\"hi\")\n  x := fs.exists(\"a.txt\")\n  assert x || !x\n}\n";
    save(&d.join("m.strata"), src);
    let (ok, out) = strata(&["--file", "m.strata", "render"], &d, None);
    assert!(ok && out == src, "{out}");
    for args in [&["--file", "m.strata", "test"][..], &["--file", "m.strata", "test", "--interpret"][..]] {
        let (ok, out) = strata(args, &d, None);
        assert!(ok && out.contains("1 passed"), "{args:?}: {out}");
    }
    let (_, out) = strata(&["--file", "m.strata", "verify"], &d, None);
    assert!(out.contains("assumed: `MemDisk` is a mock") && !out.contains("may not terminate"), "{out}");
    let (ok, out) = strata(&["--file", "m.strata", "view", "--level", "0"], &d, None);
    assert!(ok && !out.contains("type   MemDisk") && out.contains("save_then_load"), "{out}");
    let (ok, out) = strata(&["--file", "m.strata", "view", "--level", "0", "--mocks"], &d, None);
    assert!(ok && out.contains("type   MemDisk") && out.contains("impl std.fs.Files for MemDisk"), "{out}");
    // for tests only
    save(&d.join("leak.strata"), &format!("{src}\nfn leak() -> Int {{\n  len(MemDisk {{}}.files)\n}}\n"));
    let (ok, out) = strata(&["--file", "leak.strata", "check"], &d, None);
    assert!(!ok && out.contains("error in leak: `MemDisk` is a mock: it is for tests only"), "{out}");
    // a mock holds data: a record
    let (ok, out) = strata(&["--file", "x.strata", "edit"], &d, Some("mock M = Int\n"));
    assert!(!ok && out.contains("a mock holds the data of a fake"), "{out}");
}

/// A trait method named like a builtin (`find`) is the trait's only on a
/// type that implements the trait: on a list, `xs.find(…)` is still the
/// builtin (it used to be the trait's, for every receiver).
#[test]
fn a_trait_method_named_like_a_builtin_leaves_the_builtin() {
    let d = tmp("trait-builtin");
    let src = "trait Store {\n  fn find(self, k: Int) -> Option[String]\n}\n\ntype Mem = { xs: List[(Int, String)] }\n\nimpl Store for Mem {\n  fn find(self, k: Int) -> Option[String] {\n    match self.xs.find(p -> p.0 == k) {\n      Some(p) => Some(p.1),\n      None => None,\n    }\n  }\n}\n\nfn main() {\n  xs := [(1, \"a\"), (2, \"b\")]\n  print(xs.find(p -> p.0 == 2))\n  m := Mem { xs }\n  print(m.find(1))\n}\n";
    let (ok, out) = strata(&["--file", "t.strata", "edit"], &d, Some(src));
    assert!(ok, "{out}");
    let (ok, out) = strata(&["--file", "t.strata", "run"], &d, None);
    assert!(ok && out == "Some((2, \"b\"))\nSome(\"a\")\n", "{out}");
}

// A long list literal compiles in linear time: whether a later operand may
// assign was asked of the rest for each element (100,000 elements took 9 s
// through `run f ARGS`, which writes its arguments as literals; 0.1 s now).
#[test]
fn long_list_literals_compile_in_linear_time() {
    let d = tmp("long-literal");
    save(&d.join("t.strata"), "fn total(xs: List[Int]) -> Int {\n  sum(xs)\n}\n");
    // 60,000 elements: one argument stays under Linux's 128 KB (3.6 s before, 0.08 s now)
    let arg = format!("[{}]", vec!["1"; 60_000].join(","));
    // (a first run may precompile std, once per strata binary)
    let (ok, out) = strata(&["--file", "t.strata", "run", "total", "[1, 2]"], &d, None);
    assert!(ok && out.trim() == "3", "{out}");
    let start = std::time::Instant::now();
    let (ok, out) = strata(&["--file", "t.strata", "run", "total", &arg], &d, None);
    assert!(ok && out.trim() == "60000", "{out}");
    assert!(start.elapsed().as_secs_f64() < 2.0, "took {:?}", start.elapsed());
}

// `&`, `|`, `^`, `<<`, `>>` on integers, small and big, as Python's (two's
// complement, infinitely sign-extended; `>>` rounds down), with Rust's
// precedence (`&` before `==`); a negative shift is a fault, and `verify`
// finds it; a bit operation compared is shown in parentheses, which a `|`
// in a `where` pattern needs; the same operation met twice is one value to
// the solver (a refinement assumed and checked again).
#[test]
fn bit_operators() {
    let d = tmp("bits");
    save(&d.join("bits.strata"), "fn show3(a: Int, b: Int) {\n  print(f\"{a & b} {a | b} {a ^ b}\")\n}\n\nfn show2(a: Int, k: Int) {\n  print(f\"{a << k} {a >> k}\")\n}\n\nfn main() {\n  show3(12, 10)\n  show3(-12, 10)\n  show3(-1, 1180591620717411303429)\n  show3(18446744073709551619, -36893488147419103232)\n  show3(7, 0)\n  show2(1, 70)\n  show2(-5, 1)\n  show2(-5, 3)\n  show2(1180591620717411303424, 69)\n  show2(-1180591620717411303425, 68)\n  show2(3, 0)\n  print(1 + 2 << 3 & 7 | 8 ^ 1)\n  print(-1 & 255 == 255)\n}\n");
    let (ok, out) = strata(&["--file", "bits.strata", "run"], &d, None);
    assert!(ok, "{out}");
    assert_eq!(out.trim(), "8 14 6\n0 -2 -2\n1180591620717411303429 -1 -1180591620717411303430\n0 -18446744073709551613 -18446744073709551613\n0 7 7\n1180591620717411303424 0\n-10 -3\n-40 -1\n696898287454081973172991196020261297061888 2\n-348449143727040986586790745915310001356800 -5\n3 3\n9\ntrue");
    save(&d.join("neg.strata"), "fn shifted(n: Int) -> Int {\n  1 << n\n}\n\nfn low(x: Int) -> Int\n  requires x >= 0\n  ensures it <= 255\n{\n  x & 255\n}\n\nfn kind(n: Int) -> Int {\n  match n {\n    where (it | 1) == 3 | 0 => 1,\n    _ => 2,\n  }\n}\n\n\ntype Flags = Int\n  where (it | 15) == 15\n\nfn same(x: Flags) -> Flags {\n  x\n}\n");
    let (_, out) = strata(&["--file", "neg.strata", "verify", "--cases", "10"], &d, None);
    assert!(out.contains("✗ shifted") && out.contains("by a negative number of bits") && out.contains("✓ low") && out.contains("✓ kind") && out.contains("✓ same"), "{out}");
    let (_, out) = strata(&["--file", "neg.strata", "render"], &d, None);
    // a bit operation compared keeps its parentheses (C reads `x | 15 == 15` otherwise)
    assert!(out.contains("where (it | 1) == 3 | 0 => 1,") && out.contains("where (it | 15) == 15") && out.contains("x & 255"), "{out}");
}
