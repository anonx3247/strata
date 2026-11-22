//! `compiler/builtins` holds the signatures of `src/types.rs`'s `BUILTINS`:
//! printed as text (the compiler package's `builtins` command), they are
//! `strata builtins`' lines, in the same order.

use std::process::Command;

#[path = "common/compiler.rs"]
mod compiler;

const STRATA: &str = env!("CARGO_BIN_EXE_strata");

#[test]
fn builtins_as_in_rust() {
    let got = compiler::run(&["builtins"]);
    let o = Command::new(STRATA).arg("builtins").output().unwrap();
    let all = String::from_utf8_lossy(&o.stdout).to_string();
    let (_, listed) = all.split_once("// builtin functions\n\n").expect("strata builtins lists the builtin functions");
    assert!(listed.lines().count() > 50, "{all}");
    assert_eq!(got, listed);
}
