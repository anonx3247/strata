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
    for f in RUNTIME_FILES {
        println!("cargo:rerun-if-changed={f}");
    }
    let fresh = std::fs::read_to_string(core_rt).ok().filter(|ir| ir.lines().next() == Some(&format!("; core.rt {}", core_stamp())));
    let mut b = cc::Build::new();
    b.file("src/llvm/runtime.c").opt_level(2);
    if fresh.is_some() {
        b.define("STRATA_CORE_RT", None).file(core_rt);
    } else if core_rt.exists() {
        println!("cargo:warning=target/core_rt.ll is older than core's runtime files: strata keeps the C runtime's versions (run tools/bootstrap.sh)");
    }
    b.compile("strata_runtime");
    // std's precompiled library (src/llvm/jit.rs, `std_library`) calls the
    // runtime: strata exports it, so the library's calls are to this one
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => println!("cargo:rustc-link-arg-bins=-Wl,-export_dynamic"),
        Ok("linux") => println!("cargo:rustc-link-arg-bins=-Wl,--export-dynamic"),
        _ => {}
    }
}

/// core.mem's and the runtime's module files (`core::RUNTIME`), in the order
/// of `core::FILES`: what core_rt.ll is made from.
const RUNTIME_FILES: [&str; 6] = ["core/mem.strata", "core/rt.strata", "core/str.strata", "core/int.strata", "core/list.strata", "core/fmt.strata"];

/// FNV-1a over `RUNTIME_FILES`, as `strata build-core`
/// stamps its output (src/cmd/build.rs, `core_stamp`).
fn core_stamp() -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for f in RUNTIME_FILES {
        for b in std::fs::read(f).unwrap_or_default() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    format!("{h:016x}")
}
