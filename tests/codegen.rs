//! `compiler/codegen` (the compiler package's `codegen` command), the code
//! generator written in Strata: each program here is built by `strata
//! build` and by the Strata code generator (its IR linked with the same C
//! runtime by clang), and the two executables must print the same things and
//! end the same way (faults included). The IR itself may differ.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const STRATA: &str = env!("CARGO_BIN_EXE_strata");

#[path = "common/compiler.rs"]
mod compiler;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("strata-codegen-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn cc() -> String {
    std::env::var("STRATA_CC").unwrap_or_else(|_| "clang".into())
}

fn have_clang() -> bool {
    Command::new(cc()).arg("--version").output().is_ok_and(|o| o.status.success())
}

/// Store a program written as text at `file` (a module file).
fn save(file: &Path, text: &str) {
    let mut c = Command::new(STRATA).args(["--file", &file.to_string_lossy(), "edit"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success(), "{}: {}", file.display(), String::from_utf8_lossy(&o.stderr));
}

/// What an executable printed and how it ended.
fn outcome(exe: &Path) -> (String, String, Option<i32>) {
    let o = Command::new(exe).output().unwrap();
    (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned(), o.status.code())
}

/// The program at `file` built by Rust and by Strata: their outcomes.
fn both(dir: &Path, name: &str, file: &Path) -> ((String, String, Option<i32>), (String, String, Option<i32>)) {
    let rust = dir.join(format!("{name}-rust"));
    let o = Command::new(STRATA).arg("build").arg(file).arg("-o").arg(&rust).output().unwrap();
    assert!(o.status.success(), "{name}: strata build: {}", String::from_utf8_lossy(&o.stderr));
    let ir = compiler::run(&["codegen".to_string(), file.to_string_lossy().into_owned()]);
    assert!(!ir.starts_with("error: "), "{name}: not compiled by compiler/codegen:\n{ir}");
    let ll = dir.join(format!("{name}.ll"));
    std::fs::write(&ll, &ir).unwrap();
    let ours = dir.join(format!("{name}-strata"));
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/llvm/runtime.c");
    let o = Command::new(cc()).args(["-O2", "-Wno-override-module", "-o"]).arg(&ours).arg(&ll).arg(&runtime).arg("-lm").output().unwrap();
    assert!(o.status.success(), "{name}: clang refused compiler/codegen's IR: {}", String::from_utf8_lossy(&o.stderr));
    (outcome(&rust), outcome(&ours))
}

/// Programs of the constructs step 1 compiles, each ending as it ends
/// (normally, with an exit code, or with a fault).
const PROGRAMS: &[(&str, &str)] = &[
    (
        "arithmetic",
        "fn fib(n: Int) -> Int {
    if n < 2 {
        n
    } else {
        fib(n - 1) + fib(n - 2)
    }
}

fn main() {
    print(fib(20))
    print(7 / 2)
    print(-7 / 2)
    print(-7 % 3)
    print(-(3 * 4) + 1)
    print(6 & 3)
    print(6 | 3)
    print(6 ^ 3)
    print(1 << 10)
    print(1024 >> 3)
    print(2.5 * 4.0 - 1.0 / 4.0)
    print(7.5 % 2.0)
    print(-1.5)
    print(1 == 1 && 2 != 3)
    print(1 > 2 || 2 >= 2)
    print(false ==> 1 / 0 == 0)
    print(0.1 + 0.2 == 0.3)
    print(1.5 <= 1.5)
}
",
    ),
    (
        "big_integers",
        "fn factorial(n: Int) -> Int {
    mut acc := 1
    mut i := 2
    while i <= n {
        acc = acc * i
        i += 1
    }
    acc
}

fn main() {
    big := factorial(30)
    print(big)
    print(big / factorial(28))
    print(big - big + 1)
    print(big > factorial(29))
    print(big == factorial(30))
    print(4611686018427387904 - 1)
    print(-4611686018427387904 - 1)
    print(123456789012345678901234567890)
    print((1 << 70) | 1)
    print((1 << 70) & ((1 << 70) + 5))
    print((1 << 70) ^ 3)
}
",
    ),
    (
        "strings",
        "fn greet(name: String) -> String {
    \"hello, \" + name
}

fn main() {
    s := greet(\"Ada\")
    print(s)
    t := \"hello, Ada\"
    print(f\"{s}! {1 + 2} {2.5} {true} {s == t}\")
    print(\"quote \\\" and backslash \\\\ and é\")
    print(\"\")
}
",
    ),
    (
        "loops",
        "fn first_square_over(n: Int) -> Int {
    mut i := 0
    loop {
        if i * i > n {
            return i
        }
        i += 1
    }
    0
}

fn main() {
    mut total := 0
    mut i := 0
    while i < 10 {
        i += 1
        if i % 2 == 0 {
            continue
        }
        if i > 7 {
            break
        }
        total += i
    }
    print(total)
    print(first_square_over(50))
    mut n := 0
    loop {
        n += 3
        if n > 10 {
            break
        }
    }
    print(n)
}
",
    ),
    (
        "exit_code",
        "fn main() -> Int {
    x := 40
    {
        x := 1
        print(x)
    }
    if x > 10 {
        return x + 2
    }
    0
}
",
    ),
    (
        "assert_fails",
        "fn check(a: Int, b: String) {
    assert b == \"b\"
    assert a + 1 < 3
}

fn main() {
    check(1, \"b\")
    print(\"one\")
    check(5, \"b\")
    print(\"never\")
}
",
    ),
    (
        "assert_strings",
        "fn main() {
    assert \"x\" + \"y\" == \"xz\"
}
",
    ),
    (
        "division_by_zero",
        "fn divide(a: Int, b: Int) -> Int {
    a / b
}

fn main() {
    print(divide(7, 2))
    print(divide(1, 0))
}
",
    ),
    (
        "float_overflow",
        "fn main() {
    mut x := 1.5
    while true {
        x = x * 10000000000.0
        print(x)
    }
}
",
    ),
];

#[test]
fn compiled_programs_do_what_rust_compiled_ones_do() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let dir = scratch("programs");
    for (name, text) in PROGRAMS {
        let file = dir.join(format!("{name}.strata"));
        save(&file, text);
        let (rust, ours) = both(&dir, name, &file);
        assert_eq!(ours, rust, "{name}: (stdout, stderr, exit code) of compiler/codegen's build vs strata build's");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn refuses_what_it_does_not_compile_yet() {
    let dir = scratch("refused");
    let file = dir.join("records.strata");
    save(&file, "type P = { x: Int }\n\nfn main() {\n    p := P { x: 1 }\n    print(p.x)\n}\n");
    let out = compiler::run(&["codegen".to_string(), file.to_string_lossy().into_owned()]);
    assert!(out.starts_with("error: in `main`: not compiled yet: "), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}
