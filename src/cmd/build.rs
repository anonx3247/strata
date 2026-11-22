//! Native compilation: build.

use crate::*;

/// `strata build [PATH] [-o OUT] [--emit-llvm]`: compile the program's `main`
/// to LLVM IR, then (unless --emit-llvm) to an executable with clang.
pub fn build(args: &Args) -> Result<bool, String> {
    let (src, path) = match args.pos.get(1) {
        Some(f) => (Source::File(read_sources(f)?), f.clone()),
        None => (load(args)?, args.val("--file").unwrap_or(".").to_string()),
    };
    let p = src.program();
    let rep = full_check(p);
    if !rep.ok() {
        print_errors(&rep);
        return Ok(false);
    }
    // the checks the last `verify` proved are left out (unless --all-checks)
    let file = args.pos.get(1).map(|s| s.as_str()).or(args.val("--file"));
    let proofs = proofs_for(&src, file, args);
    let used = !proofs.is_empty();
    let (ir, left_out, kept) = llvm::compile_proven(p, "main", proofs, false)?;
    if args.flag("--emit-llvm") {
        match args.val("-o") {
            Some(out) => std::fs::write(out, &ir).map_err(|e| format!("{out}: {e}"))?,
            None => print!("{ir}"),
        }
        return Ok(true);
    }
    // the executable is named after the package, or the file
    let path = std::path::Path::new(&path);
    let pkg = package::find(path)?;
    let name = match &pkg {
        Some(pkg) => pkg.name.clone(),
        None => path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "main".into()),
    };
    // the package's C, for its `extern c` declarations
    let mut native = pkg.as_ref().map(|p| p.native.clang_args(&p.dir)).unwrap_or_default();
    native.retain(|a| a != "-lm"); // always linked
    let out = args.val("-o").map(String::from).unwrap_or(name);
    let dir = std::env::temp_dir().join(format!("strata-build-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (ll, rt) = (dir.join("program.ll"), dir.join("runtime.c"));
    std::fs::write(&ll, &ir).map_err(|e| e.to_string())?;
    std::fs::write(&rt, llvm::RUNTIME).map_err(|e| e.to_string())?;
    let cc = std::env::var("STRATA_CC").unwrap_or_else(|_| "clang".into());
    let status = Command::new(&cc)
        .args(["-O2", "-Wno-override-module", "-o", &out])
        .arg(&ll)
        .arg(&rt)
        // std's precompiled code, linked in (always statically: an executable stands alone)
        .args(llvm::jit::std_library().map(|l| l.object.clone()))
        .args(&native)
        .arg("-lm")
        .status()
        .map_err(|e| format!("can't run `{cc}` (a C compiler with LLVM, e.g. clang; set STRATA_CC to choose one): {e}"));
    let _ = std::fs::remove_dir_all(&dir);
    if !status?.success() {
        return Err(format!("`{cc}` failed on the generated code"));
    }
    if used {
        eprintln!("built {out}: {left_out} runtime checks left out (proven by `strata verify`), {kept} kept");
    } else {
        eprintln!("built {out}");
    }
    Ok(true)
}
