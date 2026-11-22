//! `strata check --compare`: the Rust checker's errors next to those of the
//! checker written in Strata (the `compiler/` package's `check` command),
//! built once into the cache.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::types::CheckReport;

/// Print the errors only one of the checkers finds on the script `file`
/// (`rep` is the Rust checker's report). Whether they agree.
pub fn compare(file: &str, rep: &CheckReport) -> Result<bool, String> {
    if !Path::new(file).is_file() {
        return Err("--compare checks a script (one file given with --file) for now, not a folder or package".into());
    }
    let dir = compiler_dir().ok_or("--compare: no compiler package found (a `compiler/` folder holding strata.toml in this folder or above, or next to the strata executable; or set STRATA_COMPILER to its path)")?;
    let exe = built(&dir)?;
    let mut cmd = Command::new(&exe);
    cmd.arg("check");
    let std_dir = std_dir(&dir);
    if let Some(s) = &std_dir {
        cmd.arg("--std").arg(s);
    }
    let o = cmd.arg(file).output().map_err(|e| format!("--compare: running {}: {e}", exe.display()))?;
    if !o.status.success() {
        return Err(format!("--compare: the Strata checker failed ({}): {}", o.status, String::from_utf8_lossy(&o.stderr).trim()));
    }
    let mut strata: Vec<String> = String::from_utf8_lossy(&o.stdout).lines().filter(|l| l.starts_with("error in ")).map(String::from).collect();
    let mut rust: Vec<String> = rep.errors.iter().map(|e| format!("error in {}: {}", e.decl, e.msg)).collect();
    let n = rust.len();
    // the same lines, in any order: each one matched once
    rust.retain(|l| match strata.iter().position(|s| s == l) {
        Some(i) => {
            strata.remove(i);
            false
        }
        None => true,
    });
    for l in &rust {
        println!("only in rust: {l}");
    }
    for l in &strata {
        println!("only in strata: {l}");
    }
    if rust.is_empty() && strata.is_empty() {
        println!("compare: the checkers agree ({n} errors)");
        Ok(true)
    } else {
        println!("compare: {} only in rust, {} only in strata ({} agree)", rust.len(), strata.len(), n - rust.len());
        Ok(false)
    }
}

/// The compiler package: `STRATA_COMPILER`, else a `compiler/` folder (with
/// strata.toml) in the current folder or above, else beside the executable
/// (`target/release/strata` in the repository).
fn compiler_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("STRATA_COMPILER") {
        let d = PathBuf::from(d);
        return d.join("strata.toml").is_file().then_some(d);
    }
    let found = |start: &Path| start.ancestors().map(|a| a.join("compiler")).find(|c| c.join("strata.toml").is_file());
    std::env::current_dir().ok().and_then(|c| found(&c)).or_else(|| std::env::current_exe().ok().and_then(|e| found(&e)))
}

/// The std folder beside the compiler package (the repository's `std/`).
fn std_dir(compiler: &Path) -> Option<PathBuf> {
    let s = compiler.canonicalize().ok()?.parent()?.join("std");
    s.is_dir().then_some(s)
}

/// The compiler package built (`strata build`), kept in
/// `~/.cache/strata/compiler/<hash>` (`STRATA_CACHE`): the hash covers the
/// package's and std's files and this executable, so a change rebuilds it.
fn built(dir: &Path) -> Result<PathBuf, String> {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    let mut files = vec![];
    sources(dir, dir, &mut files);
    if let Some(s) = std_dir(dir) {
        sources(&s, &s, &mut files);
    }
    files.sort();
    for (rel, path) in &files {
        h.update(rel.as_bytes());
        h.update([0]);
        h.update(fs::read(path).unwrap_or_default());
        h.update([0]);
    }
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    if let Ok(m) = fs::metadata(&me) {
        h.update(format!("{}:{}:{:?}", me.display(), m.len(), m.modified().ok()).as_bytes());
    }
    let key: String = h.finalize().iter().take(16).map(|b| format!("{b:02x}")).collect();
    let root = match std::env::var_os("STRATA_CACHE") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(std::env::var_os("HOME").ok_or("--compare: no HOME for the cache")?).join(".cache/strata"),
    };
    let out = root.join("compiler").join(&key);
    let exe = out.join("compiler");
    if exe.is_file() {
        return Ok(exe);
    }
    fs::create_dir_all(&out).map_err(|e| format!("--compare: {}: {e}", out.display()))?;
    eprintln!("building the Strata checker ({}) into {} ...", dir.display(), out.display());
    let tmp = out.join(format!("compiler.{}", std::process::id()));
    let o = Command::new(&me).arg("build").arg("--no-verify").arg(dir).arg("-o").arg(&tmp).output().map_err(|e| e.to_string())?;
    if !o.status.success() {
        let _ = fs::remove_file(&tmp);
        return Err(format!("--compare: strata build {} failed:\n{}{}", dir.display(), String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)));
    }
    fs::rename(&tmp, &exe).map_err(|e| e.to_string())?;
    Ok(exe)
}

/// The `.strata` files and settings under `dir` (not hidden folders such as
/// the package's cache), with their paths relative to `root`.
fn sources(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if p.is_dir() {
            sources(root, &p, out);
        } else if name.ends_with(".strata") || name == "strata.toml" || name == "strata.lock" {
            out.push((p.strip_prefix(root).unwrap_or(&p).to_string_lossy().into_owned(), p));
        }
    }
}
