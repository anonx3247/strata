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
    // what the compiler is made of, for keys of results that only change with
    // it (verify's kept verdicts): its sources and dependencies, not std, which
    // the results key by its definitions' hashes
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.lock");
    let mut files = vec![std::path::PathBuf::from("Cargo.lock"), "build.rs".into()];
    let mut dirs = vec![std::path::PathBuf::from("src")];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() { dirs.push(p) } else { files.push(p) }
        }
    }
    files.sort();
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    for f in &files {
        f.hash(&mut h);
        std::fs::read(f).unwrap_or_default().hash(&mut h);
    }
    println!("cargo:rustc-env=STRATA_SOURCE_HASH={:016x}", h.finish());
}
