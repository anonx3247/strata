// With the `jit` feature, the native runtime (src/llvm/runtime.c) is compiled
// into strata itself, so JIT-compiled code calls the same runtime as a binary
// from `strata build`.
fn main() {
    println!("cargo:rerun-if-changed=src/llvm/runtime.c");
    #[cfg(feature = "jit")]
    cc::Build::new().file("src/llvm/runtime.c").opt_level(2).compile("strata_runtime");
}
