// The native runtime (src/llvm/runtime.c) is compiled
// into strata itself, so JIT-compiled code calls the same runtime as a binary
// from `strata build`.
fn main() {
    println!("cargo:rerun-if-changed=src/llvm/runtime.c");
    cc::Build::new().file("src/llvm/runtime.c").opt_level(2).compile("strata_runtime");
    // std's precompiled library (src/llvm/jit.rs, `std_library`) calls the
    // runtime: strata exports it, so the library's calls are to this one
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => println!("cargo:rustc-link-arg-bins=-Wl,-export_dynamic"),
        Ok("linux") => println!("cargo:rustc-link-arg-bins=-Wl,--export-dynamic"),
        _ => {}
    }
}
