//! `strata build`: a compiled program prints what the interpreter prints,
//! faults included, and exits with the same code.

use std::path::{Path, PathBuf};
use std::process::Command;

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

/// Build `file` natively, run it, and run it in the interpreter: same output.
fn same_as_interpreter(file: &Path, dir: &Path) -> (String, i32) {
    let strata = env!("CARGO_BIN_EXE_strata");
    let exe = dir.join("prog");
    let b = Command::new(strata).arg("build").arg(file).arg("-o").arg(&exe).output().unwrap();
    assert!(b.status.success(), "build failed: {}", String::from_utf8_lossy(&b.stderr));
    let native = output(&mut Command::new(&exe));
    let interp = output(Command::new(strata).arg("run").arg(file));
    assert_eq!(native, interp, "native (left) and interpreted (right) runs differ for {}", file.display());
    native
}

fn case(dir: &Path, name: &str, src: &str) -> (String, i32) {
    let f = dir.join(format!("{name}.strata"));
    std::fs::write(&f, src).unwrap();
    same_as_interpreter(&f, dir)
}

#[test]
fn numbers_compile_and_run_as_interpreted() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let d = tmp("numbers");
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/build/numbers.strata");
    let (out, code) = same_as_interpreter(&file, &d);
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
    let cases = [
        ("early_return", "fn sign(n: Int) -> Int { if n < 0 { return -1 } else { 0 } }\nfn main() -> () { print(sign(-5)) print(sign(5)) }", 0),
        ("requires", "fn half(n: Int) -> Int requires n % 2 == 0 { n / 2 }\nfn main() -> () { print(half(4)) print(half(3)) }", 101),
        ("ensures", "fn f(x: Int) -> Int ensures it > x { x }\nfn main() -> () { print(f(2)) }", 101),
        ("assert", "fn main() -> () { let s = \"a\\\"b\" assert s + \"!\" == \"x\" }", 101),
        ("assignment", "fn main() -> () { let mut x: Int where it < 5 = 0 while true { x = x + 1 } }", 101),
        ("division", "fn main() -> () { print(-7 / 2) print(-7 % 2) print(7 / -2) print(1 / 0) }", 101),
        ("floats", "fn main() -> () { print(0.1 + 0.2) print(1000000000000000000000.0) print(0.00000015) print(100.0) print(-5.5 % 2.0) print(1.0 / 0.0) }", 101),
        ("exit_code", "fn main() -> Int { 3 }", 3),
        (
            "loops",
            "fn g(n: Int) -> Int { let mut i = 0 while i < 10 { i = i + 1 if i == n { break } if i % 2 == 0 { continue } } i }\n\
             fn pick(b: Bool) -> String { match b { true => \"yes\", false => return \"no\" } }\n\
             fn main() -> () { print(g(4)) print(g(40)) print(pick(true) + pick(false)) print(f\"{1.5} {true} {\"q\"} {len(\"héllo\")}\") }",
            0,
        ),
    ];
    for (name, src, want) in cases {
        let (out, code) = case(&d, name, src);
        assert_eq!(code, want, "{name}: {out}");
    }
}

#[test]
fn what_is_not_compiled_yet_is_named() {
    let d = tmp("unsupported");
    let f = d.join("lists.strata");
    std::fs::write(&f, "fn total(xs: List[Int]) -> Int { sum(xs) }\nfn main() -> () { print(total([1, 2])) }\n").unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_strata")).arg("build").arg(&f).arg("--emit-llvm").output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success() && err.contains("can't compile `main`: a list is not compiled yet"), "{err}");
}

#[test]
fn emits_llvm_ir() {
    let d = tmp("emit");
    let f = d.join("p.strata");
    std::fs::write(&f, "fn twice(n: Int) -> Int { n * 2 }\nfn main() -> () { print(twice(21)) }\n").unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_strata")).arg("build").arg(&f).arg("--emit-llvm").output().unwrap();
    let ir = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(ir.contains("define i64 @\"strata:twice\"(i64 %p.n)") && ir.contains("define i32 @main()"), "{ir}");
}
