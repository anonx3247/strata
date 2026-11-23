//! The compiler written in Strata (the `compiler/` package), built once per
//! test binary: its `main` runs one step of the front end on files
//! (`lex FILE...`, `check FILE...`, ...), for the tests to compare with the
//! Rust front end.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The package built (`strata build compiler -o EXE`): the executable.
pub fn exe() -> &'static Path {
    static EXE: OnceLock<PathBuf> = OnceLock::new();
    EXE.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let dir = std::env::temp_dir().join(format!("strata-compiler-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("compiler");
        let o = Command::new(env!("CARGO_BIN_EXE_strata")).arg("build").arg(root.join("compiler")).arg("-o").arg(&exe).output().unwrap();
        assert!(o.status.success(), "strata build compiler ({}): {}{}", o.status, String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        exe
    })
}

/// The compiler's output for `args` (a command, then its arguments).
#[allow(dead_code)]
pub fn run<S: AsRef<std::ffi::OsStr>>(args: &[S]) -> String {
    let o = Command::new(exe()).args(args).output().unwrap();
    assert!(o.status.success(), "compiler {:?}: {}", args.iter().map(|a| a.as_ref().to_string_lossy().into_owned()).collect::<Vec<_>>(), String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}
