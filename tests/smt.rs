//! `compiler/smt` (the compiler package's `smt` command), the verifier's
//! SMT encoding written in Strata: for each function it covers, the
//! solver's answer to each obligation must be the one `strata verify
//! --obligations` gets (proven, refuted or unknown), obligation for
//! obligation. A function using what it doesn't model yet says `not
//! covered` and is left out of the comparison.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const STRATA: &str = env!("CARGO_BIN_EXE_strata");

#[path = "common/compiler.rs"]
mod compiler;

fn have_z3() -> bool {
    Command::new("z3").arg("--version").output().is_ok_and(|o| o.status.success())
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("strata-smt-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Store a program written as text at `file` (a module file).
fn save(file: &Path, text: &str) {
    let mut c = Command::new(STRATA).args(["--file", &file.to_string_lossy(), "edit"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success(), "{}: {}", file.display(), String::from_utf8_lossy(&o.stderr));
}

/// Each function's lines (`fn NAME`, then its obligations' outcomes).
fn functions(out: &str) -> Vec<(String, Vec<String>)> {
    let mut fns: Vec<(String, Vec<String>)> = vec![];
    for l in out.lines() {
        if let Some(n) = l.strip_prefix("fn ") {
            fns.push((n.to_string(), vec![]));
        } else if let (Some(line), Some(f)) = (l.strip_prefix("  "), fns.last_mut()) {
            f.1.push(line.to_string());
        }
    }
    fns
}

/// The script at `file` through both verifiers: the functions
/// `compiler/smt` covers, each with its outcomes from Rust and from Strata.
fn both(file: &Path, cache: &Path) -> (Vec<(String, Vec<String>, Vec<String>)>, usize) {
    let o = Command::new(STRATA).args(["--file", &file.to_string_lossy(), "verify", "--obligations"]).output().unwrap();
    assert!(o.status.success(), "{}: strata verify --obligations: {}", file.display(), String::from_utf8_lossy(&o.stderr));
    let rust = functions(&String::from_utf8_lossy(&o.stdout));
    let o = Command::new(compiler::exe()).arg("smt").arg(file).env("STRATA_CACHE", cache).output().unwrap();
    assert!(o.status.success(), "{}: compiler smt: {}", file.display(), String::from_utf8_lossy(&o.stderr));
    let ours = functions(&String::from_utf8_lossy(&o.stdout));
    let all = ours.len();
    let covered = ours
        .into_iter()
        .filter(|(_, ls)| !ls.first().is_some_and(|l| l.starts_with("not covered")))
        .map(|(n, ls)| {
            let r = rust.iter().find(|f| f.0 == n).map(|f| f.1.clone()).unwrap_or_else(|| panic!("{}: `{n}` is not among Rust's functions", file.display()));
            (n, r, ls)
        })
        .collect();
    (covered, all)
}

/// The functions covered (all agreeing), and all of them.
fn same(file: &Path, cache: &Path) -> (usize, usize) {
    let (fns, all) = both(file, cache);
    for (n, r, s) in &fns {
        assert_eq!(r, s, "{}: `{n}`: Rust's outcomes, then compiler/smt's", file.display());
    }
    (fns.len(), all)
}

/// Each step's program: every function of it is covered, and agrees.
/// Step 1: integers, booleans and strings, their refinements, operators,
/// `if`, blocks, variables, `assert`, `return` and calls through contracts.
/// Step 2: records, sums (`Option`, `Result`), tuples and distinct types as
/// datatypes; constructors, fields, updates, `match` and patterns, `is`,
/// `!` / `?`, `except` / `??`.
/// Step 3: lists (and dicts and sets by their size): literals, indexing,
/// slices, `+`, list patterns, element types, lambdas passed to `map`,
/// `filter`, `all`, `any`, `find`; the builtins on lists, strings and
/// numbers.
/// Step 4: `while` (invariants inferred and written), `for` over lists,
/// `loop`, `break`, `continue`.
/// Step 5: method calls (through the one trait declaring them, else any
/// value of their result), function names as values, generic functions;
/// calls seen through in contracts (step 6).
/// Step 7: `matches` with a literal pattern (std.regex's SMT translation).
/// Step 13: union values: values of different shapes merged, union types,
/// patterns naming a type, `is` with a type, fields and equality of unions.
/// Step 14: calls seen through in contracts when whether they finish needs
/// the solver (a `while` loop's bound, the measure of a recursive function
/// passed as a value) or can't be shown (`loop`, an `Iterator`).
#[test]
fn smt_outcomes_as_in_rust() {
    if !have_z3() {
        eprintln!("skipped: no z3");
        return;
    }
    let dir = scratch("steps");
    for (name, text) in [
        ("step1", include_str!("data/smt/step1.st")),
        ("step2", include_str!("data/smt/step2.st")),
        ("step3", include_str!("data/smt/step3.st")),
        ("step4", include_str!("data/smt/step4.st")),
        ("step5", include_str!("data/smt/step5.st")),
        ("step7", include_str!("data/smt/step7.st")),
        ("step10", include_str!("data/smt/step10.st")),
        ("step13", include_str!("data/smt/step13.st")),
        ("step14", include_str!("data/smt/step14.st")),
    ] {
        let file = dir.join(format!("{name}.strata"));
        save(&file, text);
        let (covered, all) = same(&file, &dir);
        assert_eq!(covered, all, "{name}: every function is covered");
    }
}

/// The repository's scripts: the functions `compiler/smt` covers agree.
#[test]
fn smt_outcomes_on_scripts_as_in_rust() {
    if !have_z3() {
        eprintln!("skipped: no z3");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("scripts");
    let mut covered = 0;
    for d in ["examples", "tests/data"] {
        let mut files: Vec<PathBuf> = std::fs::read_dir(root.join(d)).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "strata")).collect();
        files.sort();
        for f in files {
            covered += same(&f, &dir).0;
        }
    }
    assert!(covered > 0, "no function of the scripts is covered");
}

/// std's modules (a few, quick to verify): the functions `compiler/smt`
/// covers agree, named as Rust names them (`std.utf8.encode`).
#[test]
fn smt_outcomes_on_std_as_in_rust() {
    if !have_z3() {
        eprintln!("skipped: no z3");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("std");
    for m in ["utf8", "time", "http", "process"] {
        let o = Command::new(STRATA).current_dir(root).args(["--file", "std", "verify", &format!("std.{m}.*"), "--obligations"]).output().unwrap();
        assert!(o.status.success(), "std.{m}: {}", String::from_utf8_lossy(&o.stderr));
        let rust = functions(&String::from_utf8_lossy(&o.stdout));
        let o = Command::new(compiler::exe()).current_dir(root).arg("smt").arg(format!("std/{m}.strata")).env("STRATA_CACHE", &dir).output().unwrap();
        assert!(o.status.success(), "compiler smt std/{m}: {}", String::from_utf8_lossy(&o.stderr));
        let ours = functions(&String::from_utf8_lossy(&o.stdout));
        assert!(!ours.is_empty(), "std.{m}: no function");
        for (n, ls) in ours {
            if ls.first().is_some_and(|l| l.starts_with("not covered")) {
                continue;
            }
            let r = rust.iter().find(|f| f.0 == n).map(|f| f.1.clone()).unwrap_or_else(|| panic!("std.{m}: `{n}` is not among Rust's functions"));
            assert_eq!(r, ls, "std.{m}: `{n}`: Rust's outcomes, then compiler/smt's");
        }
    }
}

/// `compiler termination` (the termination analysis written in Strata,
/// `compiler/termination`): each function's and implementation method's
/// verdict, and whether a contract may see through it, as `strata verify
/// --termination` gives them, on the step programs (and verify.st:
/// `to_string(x)` is `Display`'s method, not a builtin), the repository's
/// scripts and a few std modules.
#[test]
fn termination_as_in_rust() {
    if !have_z3() {
        eprintln!("skipped: no z3");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("termination");
    let mut files: Vec<(PathBuf, Option<String>)> = vec![];
    for (name, text) in [("step4", include_str!("data/smt/step4.st")), ("step5", include_str!("data/smt/step5.st")), ("termination", include_str!("data/smt/termination.st")), ("verify", include_str!("data/smt/verify.st"))] {
        let file = dir.join(format!("{name}.strata"));
        save(&file, text);
        files.push((file, None));
    }
    for d in ["examples", "tests/data"] {
        let mut found: Vec<PathBuf> = std::fs::read_dir(root.join(d)).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "strata")).collect();
        found.sort();
        files.extend(found.into_iter().map(|f| (f, None)));
    }
    for m in ["utf8", "json", "regex", "unicode"] {
        files.push((root.join(format!("std/{m}.strata")), Some(format!("std.{m}.*"))));
    }
    for (f, std_module) in files {
        let rust = match &std_module {
            Some(sel) => Command::new(STRATA).current_dir(root).args(["--file", "std", "verify", sel, "--termination"]).output().unwrap(),
            None => Command::new(STRATA).args(["--file", &f.to_string_lossy(), "verify", "--termination"]).output().unwrap(),
        };
        assert!(rust.status.success(), "{}: strata verify --termination: {}", f.display(), String::from_utf8_lossy(&rust.stderr));
        let ours = Command::new(compiler::exe()).current_dir(root).arg("termination").arg(&f).env("STRATA_CACHE", &dir).output().unwrap();
        assert!(ours.status.success(), "{}: compiler termination: {}", f.display(), String::from_utf8_lossy(&ours.stderr));
        assert_eq!(functions(&String::from_utf8_lossy(&rust.stdout)), functions(&String::from_utf8_lossy(&ours.stdout)), "{}: Rust's verdicts, then compiler/termination's", f.display());
    }
}

/// `compiler verify` (the command written in Strata: compiler/smt's
/// answers, the solver's counterexamples replayed and the rest fuzzed in
/// the compiler's own JIT) reports what `strata verify --no-race
/// --no-cache` reports, on tests/data/smt/verify.st: proofs, counterexamples
/// from the solver (lists too) and from the fuzzer, floats left to the
/// fuzzer, preconditions, loops, recursion, `terminates`, holes, `main`.
#[test]
fn verify_as_in_rust() {
    if !have_z3() {
        eprintln!("skipped: no z3");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("verify");
    // functions; then trait methods (own, default, static), a type's own
    // methods, laws (one holding, one not) and a derived impl; then sized
    // integers (wrapped arithmetic, checked and wrapping conversions)
    for (name, text) in [("verify", include_str!("data/smt/verify.st")), ("verify_traits", include_str!("data/smt/verify_traits.st")), ("verify_sized", include_str!("data/smt/verify_sized.st"))] {
        let file = dir.join(format!("{name}.strata"));
        save(&file, text);
        let rust = Command::new(STRATA).arg("--file").arg(&file).args(["verify", "--no-race", "--no-cache"]).env("STRATA_CACHE", &dir).output().unwrap();
        let ours = Command::new(compiler::exe()).current_dir(root).arg("verify").arg(&file).env("STRATA_CACHE", &dir).output().unwrap();
        // (as Rust's: exit 1 when a counterexample is found)
        assert_eq!(ours.status.code(), rust.status.code(), "compiler verify {name}: {}", String::from_utf8_lossy(&ours.stderr));
        assert_eq!(String::from_utf8_lossy(&rust.stdout), String::from_utf8_lossy(&ours.stdout), "{name}");
    }
    // a package: its modules, a `derive CommandLine` and the copies of
    // std.cli's functions it makes (one with a counterexample)
    let hello = root.join("examples/hello");
    let rust = Command::new(STRATA).arg("--file").arg(&hello).args(["verify", "--no-race", "--no-cache"]).env("STRATA_CACHE", &dir).output().unwrap();
    let ours = Command::new(compiler::exe()).current_dir(root).args(["verify", "--no-cache"]).arg(&hello).env("STRATA_CACHE", &dir).output().unwrap();
    assert_eq!(ours.status.code(), rust.status.code(), "compiler verify hello: {}", String::from_utf8_lossy(&ours.stderr));
    assert_eq!(String::from_utf8_lossy(&rust.stdout), String::from_utf8_lossy(&ours.stdout), "hello");
}

/// `strata verify` is `compiler verify`: what it chooses (`verify NAME`,
/// `prefix*`, a module, a name that names nothing), the program checked
/// first, and a package's `[verify]` rules, as Rust's verify does them.
#[test]
fn verify_chooses_checks_and_rules_as_in_rust() {
    if !have_z3() {
        eprintln!("skipped: no z3");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("choose");
    let pkg = dir.join("pol");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("strata.toml"), "name = \"pol\"\n\n[verify]\nterminates = [\"*\"]\nno_recursion = [\"core\"]\nproven = [\"core\"]\n").unwrap();
    save(&pkg.join("core.strata"), "pub fn fact(n: Natural) -> Positive { if n == 0 { 1 } else { n * fact(n - 1) } }\npub fn spin(n: Int) -> Int {\n  mut k := n\n  loop {\n    if k == 1 { break }\n    k = k - 1\n  }\n  k\n}\n");
    save(&pkg.join("main.strata"), "from core use fact, spin\nfn fib(n: Natural) -> Natural { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }\n");
    let bad = dir.join("bad.strata");
    std::fs::write(dir.join("bad.st"), "fn f(x: Int) -> Int {\n  x + \"a\"\n}\n").unwrap();
    let _ = Command::new(STRATA).args(["--file", &bad.to_string_lossy(), "edit"]).stdin(std::fs::File::open(dir.join("bad.st")).unwrap()).output().unwrap();
    for (file, chosen) in [(&pkg, None), (&pkg, Some("core.fact")), (&pkg, Some("core")), (&pkg, Some("fi*")), (&pkg, Some("nope")), (&bad, None)] {
        let mut rust_args = vec!["--file".to_string(), file.to_string_lossy().into_owned(), "verify".into(), "--no-race".into(), "--no-cache".into(), "--cases".into(), "20".into()];
        let mut ours_args = vec!["verify".to_string(), "--no-cache".into(), "--cases".into(), "20".into()];
        if let Some(c) = chosen {
            rust_args.push(c.into());
            ours_args.extend(["--only".into(), c.into()]);
        }
        ours_args.push(file.to_string_lossy().into_owned());
        let rust = Command::new(STRATA).args(&rust_args).env("STRATA_CACHE", &dir).output().unwrap();
        let ours = Command::new(compiler::exe()).current_dir(root).args(&ours_args).env("STRATA_CACHE", &dir).output().unwrap();
        let what = format!("{} {chosen:?}", file.display());
        assert_eq!(ours.status.code(), rust.status.code(), "{what}: {}", String::from_utf8_lossy(&ours.stderr));
        assert_eq!(String::from_utf8_lossy(&rust.stdout), String::from_utf8_lossy(&ours.stdout), "{what}");
        assert_eq!(String::from_utf8_lossy(&rust.stderr), String::from_utf8_lossy(&ours.stderr), "{what}");
    }
}

/// What `compiler verify` proved is what `build` leaves out (`compiler
/// proofs`: its checks, and a function's entry checks its every caller
/// proved) for the code as it is: a function changed since loses its
/// proofs, and each definition's status (`compiler verdicts`) follows.
#[test]
fn proofs_and_statuses_follow_the_code() {
    if !have_z3() {
        eprintln!("skipped: no z3");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("proofs");
    let file = dir.join("p.strata");
    let main = "fn main() {\n  print(to_string(clamp(15, 0, 10)))\n}\n";
    save(&file, &format!("fn clamp(x: Int, lo: Int, hi: Int) -> Int\n  requires lo <= hi\n  ensures lo <= it && it <= hi\n{{\n  if x < lo {{ lo }} else if x > hi {{ hi }} else {{ x }}\n}}\n\n{main}"));
    let compiler = |args: &[&str]| -> String {
        let o = Command::new(compiler::exe()).current_dir(root).args(args).arg(&file).env("STRATA_CACHE", &dir).output().unwrap();
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    compiler(&["verify", "--quiet"]);
    let proofs = compiler(&["proofs"]);
    for line in ["proven\tclamp\tensures lo <= it && it <= hi", "proven\tmain\tprecondition of `clamp`: requires lo <= hi", "entry\tclamp\trequires 0"] {
        assert!(proofs.lines().any(|l| l == line), "{line}: {proofs}");
    }
    assert_eq!(compiler(&["verdicts"]), "clamp\tproven\nmain\tproven\n");
    // `clamp` broken: its proofs and `main`'s (which calls it) are gone,
    // until verified again, which finds the counterexample
    save(&file, "fn clamp(x: Int, lo: Int, hi: Int) -> Int\n  requires lo <= hi\n  ensures lo <= it && it <= hi\n{\n  if x < lo { lo } else { x }\n}\n");
    assert_eq!(compiler(&["proofs"]), "");
    assert_eq!(compiler(&["verdicts"]), "");
    compiler(&["verify", "--quiet"]);
    assert!(!compiler(&["proofs"]).contains("proven\tclamp"));
    assert_eq!(compiler(&["verdicts"]), "clamp\tCOUNTEREXAMPLE\nmain\tCOUNTEREXAMPLE\n");
}

/// `compiler verify` when codegen can't compile one function yet (`units`:
/// an iterator held in a record's field): the drivers are compiled together, and on an error the set
/// is split in halves until that one is alone (`compiled_drivers`, as Rust's
/// `jit_fuzzers` since #357), so the others are still fuzzed (`sq`: floats
/// aren't modelled, so it is fuzzed, not proven).
#[test]
fn verify_fuzzes_around_what_cant_be_compiled() {
    if !have_z3() {
        eprintln!("skipped: no z3");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("verify-uncompiled");
    let file = dir.join("u.strata");
    save(&file, include_str!("data/smt/verify_uncompiled.st"));
    let ours = Command::new(compiler::exe()).current_dir(root).args(["verify", "--no-cache"]).arg(&file).env("STRATA_CACHE", &dir).output().unwrap();
    let out = String::from_utf8_lossy(&ours.stdout);
    assert!(out.contains("~ sq                       fuzzed: 200 cases"), "{out}");
    assert!(out.contains("✓ a ") && out.contains("✓ b ") && out.contains("✓ c "), "{out}");
}

/// One compile names every fuzz driver that can't be compiled, those whose
/// function only calls what can't (`longer` and `longest` through `units`),
/// each with why: `verify` leaves them all out at once, not by halving the
/// drivers again and again.
#[test]
fn codegen_names_every_driver_that_cant_be_compiled() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("fuzz-uncompiled");
    let file = dir.join("u.strata");
    save(&file, include_str!("data/smt/verify_uncompiled.st"));
    let o = Command::new(compiler::exe()).current_dir(root).args(["codegen", "--fuzz-text", "a,units,longer,b,longest,sq"]).arg(&file).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    let cant: Vec<&str> = out.lines().filter(|l| l.starts_with("error: can't fuzz `")).collect();
    let why = "`: in `units`: not compiled yet: `for` over a `()`";
    assert_eq!(cant, ["units", "longer", "longest"].map(|k| format!("error: can't fuzz `{k}{why}")), "{out}");
}

/// A function taking a capability, top-level or inside another type
/// (`Option[Disk]`, a record or a sum holding one, a capability trait), is
/// not fuzzed: nothing but `strata run` makes a capability.
#[test]
fn verify_doesnt_fuzz_what_holds_a_capability() {
    if !have_z3() {
        eprintln!("skipped: no z3");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("verify-capability");
    let file = dir.join("c.strata");
    save(&file, include_str!("data/smt/verify_capability.st"));
    let ours = Command::new(compiler::exe()).current_dir(root).args(["verify", "--no-cache"]).arg(&file).env("STRATA_CACHE", &dir).output().unwrap();
    let out = String::from_utf8_lossy(&ours.stdout);
    for (f, c) in [("top", "Disk"), ("opt", "Disk"), ("held", "Disk"), ("nested", "Disk"), ("files", "Files")] {
        let line = format!("- {f:<24} skipped: it takes the capability `{c}`, which only `strata run` gives, so it isn't fuzzed");
        assert!(out.lines().any(|l| l.starts_with(&line)), "{f}: {out}");
    }
    assert!(out.contains("~ plain                    fuzzed: 200 cases"), "{out}");
}

/// `compiler verify` on examples/traits.strata reports what Rust's verify
/// does: its laws proven or refuted, and `Sku`'s values (a `distinct`
/// string matching a regex) generated from its regex's samples, not as
/// strings checked as if they were `Sku` blocks (which took gigabytes).
#[test]
fn verify_traits_example_as_in_rust() {
    if !have_z3() {
        eprintln!("skipped: no z3");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // (through `strata verify --compare`, which loads std's precompiled
    // library into the drivers: compiling std into them takes minutes)
    let file = root.join("examples/traits.strata");
    let o = Command::new(STRATA).current_dir(root).arg("--file").arg(&file).args(["verify", "--compare", "--no-cache"]).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("compare: they agree"), "{out}{}", String::from_utf8_lossy(&o.stderr));
}

/// `xs = drop(xs, 2)` under 100 `if`s: each `if` makes the list "the one
/// before, or the one before less two", and the encoder shares the one
/// before (its length, its elements, whether it is positional) instead of
/// following both branches, which doubled the work at each `if` (7 `if`s
/// went past 5 GB). It must finish in seconds.
#[test]
fn smt_shares_lists_both_branches_drop_from() {
    if !have_z3() {
        eprintln!("skipped: no z3");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("smt-ifs");
    let mut text = String::from("fn f(args: List[String]) -> String {\n  mut files := args\n");
    for k in 1..=100 {
        text += &format!("  if get(files, 0) ?? \"\" == \"--o{k}\" {{\n    files = drop(files, 2)\n  }}\n");
    }
    text += "  get(files, 0) ?? \"\"\n}\n";
    let file = dir.join("ifs.strata");
    save(&file, &text);
    let mut child = Command::new(compiler::exe()).current_dir(root).arg("smt").arg(&file).env("STRATA_CACHE", &dir).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let start = std::time::Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if start.elapsed() > std::time::Duration::from_secs(10) {
            let _ = child.kill();
            panic!("compiler smt on 100 ifs ran past 10 s");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("fn f"), "{}", String::from_utf8_lossy(&o.stdout));
}

/// `strata verify --compare` (and its other arguments) on `file`: Rust's
/// verify and `compiler verify` must agree, line for line.
fn compare(file: &Path, args: &[&str]) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let o = Command::new(STRATA).current_dir(root).arg("--file").arg(file).arg("verify").args(args).args(["--compare", "--no-cache"]).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("compare: they agree"), "{}: {out}{}", file.display(), String::from_utf8_lossy(&o.stderr));
}

/// Inputs the fuzzer can't make (a sorted list as a refinement, a
/// `requires` few values meet) are made by the solver, and the function is
/// run on them, as Rust's verify does; `s.graphemes()` is std.unicode's
/// (its `len` is `FiniteIterator`'s method); `--smt` shows termination's
/// queries too.
#[test]
fn verify_solver_inputs_graphemes_and_queries_as_in_rust() {
    if !have_z3() {
        eprintln!("skipped: no z3");
        return;
    }
    let dir = scratch("verify-parity");
    let inputs = dir.join("inputs.strata");
    save(
        &inputs,
        "type Sorted = List[Int] where len(it) >= 5 && all(1..len(it), i -> it[i - 1] < it[i])

fn zero(xs: Sorted) -> Int
  ensures it >= 0 {
  fold(xs, 0, (a, b) -> a * 1)
}

fn tight(x: Int, y: Int) -> Int
  requires x * 3 == y && 1000 < y && y < 1010
  ensures it >= 0 {
  fold([x], 0, (a, b) -> a * 1)
}
",
    );
    compare(&inputs, &[]);
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ours = Command::new(compiler::exe()).current_dir(root).args(["verify", "--no-cache"]).arg(&inputs).env("STRATA_CACHE", &dir).output().unwrap();
    let out = String::from_utf8_lossy(&ours.stdout);
    assert!(out.contains("~ zero                     fuzzed: 20 cases") && out.contains("~ tight                    fuzzed: 3 cases"), "{out}");
    let units = dir.join("units.strata");
    save(&units, "fn units(s: String) -> Natural { len(s.graphemes()) }\n");
    compare(&units, &[]);
    let terms = dir.join("terms.strata");
    save(
        &terms,
        "fn count(n: Natural) -> Natural {
  if n == 0 {
    0
  } else {
    1 + count(n - 1)
  }
}

fn up_to(n: Int) -> Int {
  mut i := 0
  while i < n {
    i += 1
  }
  i
}

fn spin(n: Int) -> Int {
  if n == 0 {
    0
  } else {
    spin(n + 1)
  }
}
",
    );
    compare(&terms, &["--smt"]);
}

/// `xs = drop(xs, 1)` under 100 `if`s, returned as a `List[Natural]`:
/// whether every element is known valid looks at each list once, not once
/// per path (both branches of each `if` reach the list before). It agrees
/// with Rust on 8 `if`s (Rust's own encoder goes past gigabytes from 20)
/// and finishes in seconds on 100.
#[test]
fn smt_valid_lists_shared_by_both_branches() {
    if !have_z3() {
        eprintln!("skipped: no z3");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("smt-valid-ifs");
    let program = |n: usize| {
        let mut text = String::from("fn f(args: List[Natural], k: Int) -> List[Natural] {\n  mut xs := args\n");
        for i in 1..=n {
            text += &format!("  if k == {i} {{\n    xs = drop(xs, 1)\n  }}\n");
        }
        text + "  xs\n}\n"
    };
    let small = dir.join("small.strata");
    save(&small, &program(8));
    compare(&small, &["--obligations"]);
    let file = dir.join("ifs.strata");
    save(&file, &program(100));
    let mut child = Command::new(compiler::exe()).current_dir(root).arg("smt").arg(&file).env("STRATA_CACHE", &dir).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let start = std::time::Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if start.elapsed() > std::time::Duration::from_secs(10) {
            let _ = child.kill();
            panic!("compiler smt on 100 ifs ran past 10 s");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let o = child.wait_with_output().unwrap();
    assert!(String::from_utf8_lossy(&o.stdout).contains("proven result of `f`: List[Natural]"), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

/// A function taking core.values' `Raw` (a compiled value's address) is
/// assumed, never run on made-up values nor on a solver's counterexample
/// (replaying one crashed on compiler/codegen's `text_at`), as Rust's
/// verify has it; a package's `[native]` paths that are absolute are kept
/// as they are (the compiler package's LLVM headers).
#[test]
fn verify_doesnt_run_raw_and_finds_absolute_native_paths() {
    if !have_z3() {
        eprintln!("skipped: no z3");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = scratch("verify-raw");
    let pkg = dir.join("rw");
    std::fs::create_dir_all(pkg.join("c")).unwrap();
    let include = dir.join("include");
    std::fs::create_dir_all(&include).unwrap();
    std::fs::write(include.join("seven.h"), "#define SEVEN 7\n").unwrap();
    std::fs::write(pkg.join("c/lib.c"), "#include <seven.h>\n#include <stdint.h>\nint64_t seven(void) { return SEVEN; }\n").unwrap();
    std::fs::write(pkg.join("strata.toml"), format!("name = \"rw\"\n\n[native]\nsources = [\"c/lib.c\"]\ninclude_dirs = [\"{}\"]\n\n[values]\nread = true\n", include.display())).unwrap();
    save(
        &pkg.join("main.strata"),
        "fn first_word(r: Raw) -> Int
  ensures it >= 0 {
  r.word(0).to_int()
}

extern c fn seven() -> I64

fn plus(n: Int) -> Int
  ensures it != n {
  n + seven().to_int()
}
",
    );
    compare(&pkg, &[]);
    let ours = Command::new(compiler::exe()).current_dir(root).args(["verify", "--no-cache"]).arg(&pkg).env("STRATA_CACHE", &dir).output().unwrap();
    let (out, err) = (String::from_utf8_lossy(&ours.stdout), String::from_utf8_lossy(&ours.stderr));
    assert!(out.contains("first_word               assumed: it reads compiled values (`Raw`)"), "{out}{err}");
    assert!(!err.contains("failed on the package's C"), "{err}");
}
