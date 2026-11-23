// The native runtime (src/llvm/runtime.c) is compiled
// into strata itself, so JIT-compiled code calls the same runtime as a binary
// from `strata build`.
fn main() {
    println!("cargo:rerun-if-changed=src/llvm/runtime.c");
    // the second stage: the runtime written in Strata (core/rt.strata), which
    // a first-stage strata compiled (`strata build-core`, tools/bootstrap.sh),
    // in place of the C runtime's versions; only while it matches core's files
    let core_rt = std::path::Path::new("target/core_rt.ll");
    println!("cargo:rerun-if-changed=target/core_rt.ll");
    println!("cargo:rerun-if-changed=core/rt.strata");
    println!("cargo:rerun-if-changed=core/mem.strata");
    let fresh = std::fs::read_to_string(core_rt).ok().filter(|ir| ir.lines().next() == Some(&format!("; core.rt {}", core_stamp())));
    let mut b = cc::Build::new();
    b.file("src/llvm/runtime.c").opt_level(2);
    if fresh.is_some() {
        b.define("STRATA_CORE_RT", None).file(core_rt);
    } else if core_rt.exists() {
        println!("cargo:warning=target/core_rt.ll is older than core/rt.strata: strata keeps the C runtime's versions (run tools/bootstrap.sh)");
    }
    b.compile("strata_runtime");
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

/// FNV-1a over core/rt.strata and core/mem.strata, as `strata build-core`
/// stamps its output (src/cmd/build.rs, `core_stamp`).
fn core_stamp() -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for f in ["core/mem.strata", "core/rt.strata"] {
        for b in std::fs::read(f).unwrap_or_default() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    format!("{h:016x}")
}
