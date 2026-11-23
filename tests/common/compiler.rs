//! The compiler written in Strata (the `compiler/` package), built once and
//! shared by every test binary and run until it or what it is built from
//! changes: its `main` runs one step of the front end on files (`lex
//! FILE...`, `check FILE...`, ...), for the tests to compare with the Rust
//! front end.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The package built (`strata build --no-verify compiler -o EXE`): the
/// executable. Tests compare behaviour, so it is built without verifying
/// it (proofs are `strata verify`'s), and kept in cargo's temporary folder
/// under a hash of strata itself and of `compiler/`, `std/` and `core/`.
pub fn exe() -> &'static Path {
    static EXE: OnceLock<PathBuf> = OnceLock::new();
    EXE.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let strata = Path::new(env!("CARGO_BIN_EXE_strata"));
        let mut h = std::collections::hash_map::DefaultHasher::new();
        // (its contents: cargo relinks it, the same bytes, on every run)
        std::fs::read(strata).unwrap_or_default().hash(&mut h);
        for dir in ["compiler", "std", "core"] {
            let mut files = vec![];
            sources(&root.join(dir), &mut files);
            files.sort();
            for f in files {
                f.strip_prefix(root).unwrap_or(&f).hash(&mut h);
                std::fs::read(&f).unwrap_or_default().hash(&mut h);
            }
        }
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("compiler").join(format!("{:016x}", h.finish()));
        let exe = dir.join("compiler");
        if exe.is_file() {
            return exe;
        }
        std::fs::create_dir_all(&dir).unwrap();
        // (built under a name of its own, then renamed: other test binaries
        // may be building it too)
        let tmp = dir.join(format!("compiler.{}", std::process::id()));
        let o = Command::new(strata).arg("build").arg("--no-verify").arg(root.join("compiler")).arg("-o").arg(&tmp).output().unwrap();
        assert!(o.status.success(), "strata build compiler ({}): {}{}", o.status, String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        std::fs::rename(&tmp, &exe).unwrap();
        exe
    })
}

/// The files under `dir` the compiler is built from (not hidden folders,
/// such as a package's cache).
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if p.is_dir() {
            sources(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// The compiler's output for `args` (a command, then its arguments).
#[allow(dead_code)]
pub fn run<S: AsRef<std::ffi::OsStr>>(args: &[S]) -> String {
    let o = Command::new(exe()).args(args).output().unwrap();
    assert!(o.status.success(), "compiler {:?}: {}", args.iter().map(|a| a.as_ref().to_string_lossy().into_owned()).collect::<Vec<_>>(), String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}
