//! Packages. A folder with a `strata.toml` is a package; its `.strata` files
//! are its modules, named after their path inside it (`shop/orders.strata`
//! is `shop.orders`, `main.strata` is the root module and the default entry
//! point). A folder below it with its own `strata.toml` is another package.
//! A `.strata` file outside every package is a standalone script.
//!
//! ```toml
//! name = "tasks"                         # kept when the folder is renamed or a worktree
//! exports = ["core", "memory.MemQueue"]  # modules or definitions; default: every `pub`
//!
//! [deps]                                 # not fetched yet
//! json = "https://example.com/strata-json"
//!
//! [verify]                               # not enforced yet
//! proven = ["core"]
//! ```

use crate::ast::Program;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const FILE: &str = "strata.toml";

pub struct Package {
    pub dir: PathBuf,
    pub name: String,
    /// `None`: every `pub` definition is visible to other packages
    pub exports: Option<Vec<String>>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    name: String,
    #[serde(default)]
    exports: Option<Vec<String>>,
    #[serde(default)]
    #[allow(dead_code)] // read when dependencies are fetched
    deps: toml::Table,
    #[serde(default)]
    #[allow(dead_code)] // read when the verifier has policies
    verify: toml::Table,
}

pub fn load(dir: &Path) -> Result<Package, String> {
    let path = dir.join(FILE);
    let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let raw: Raw = toml::from_str(&src).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Package { dir: dir.to_path_buf(), name: raw.name, exports: raw.exports })
}

/// The package a file or folder belongs to: the nearest folder at or above it
/// with a `strata.toml`.
pub fn find(start: &Path) -> Result<Option<Package>, String> {
    let start = std::fs::canonicalize(start).unwrap_or(start.to_path_buf());
    match start.ancestors().find(|d| d.join(FILE).is_file()) {
        Some(d) => load(d).map(Some),
        None => Ok(None),
    }
}

/// A new package's `strata.toml`, named after its folder.
pub fn template(dir: &Path) -> String {
    let name = std::fs::canonicalize(dir).ok().and_then(|d| d.file_name().map(|n| n.to_string_lossy().to_string())).unwrap_or_else(|| "main".into());
    format!("name = {name:?}\n")
}

/// A program's `.strata` files, relative to its folder, with their text: a
/// package's (without those of packages nested in it), or a single file as `""`.
pub fn files(path: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    if !path.is_dir() {
        let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        return Ok(vec![(PathBuf::new(), src)]);
    }
    let mut files = Vec::new();
    let mut todo = vec![path.to_path_buf()];
    while let Some(dir) = todo.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let p = entry.map_err(|e| e.to_string())?.path();
            if p.is_dir() {
                if !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) && !p.join(FILE).is_file() {
                    todo.push(p);
                }
            } else if p.extension().is_some_and(|e| e == "strata") {
                let src = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                files.push((p.strip_prefix(path).unwrap().to_path_buf(), src));
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Parse a program's files, as `files` gives them.
pub fn parse(files: Vec<(PathBuf, String)>) -> Result<Program, String> {
    match files.as_slice() {
        [(rel, src)] if rel.as_os_str().is_empty() => crate::parser::parse_module_file(src, ""),
        _ => crate::parse_files(files),
    }
}

/// What is under `top`: package folders, and `.strata` files outside every
/// package (scripts). In a git repository, ignored files are left out.
pub fn discover(top: &Path) -> Result<(Vec<PathBuf>, Vec<PathBuf>), String> {
    let listed = Command::new("git")
        .arg("-C")
        .arg(top)
        .args(["ls-files", "--cached", "--others", "--exclude-standard", "--", "*.strata", "*strata.toml"])
        .output()
        .ok()
        .filter(|o| o.status.success());
    let all: Vec<PathBuf> = match listed {
        Some(o) => String::from_utf8_lossy(&o.stdout).lines().map(|l| top.join(l)).filter(|p| p.is_file()).collect(),
        None => {
            let mut out = Vec::new();
            let mut todo = vec![top.to_path_buf()];
            while let Some(dir) = todo.pop() {
                for entry in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
                    let p = entry.map_err(|e| e.to_string())?.path();
                    if p.is_dir() {
                        if !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
                            todo.push(p);
                        }
                    } else {
                        out.push(p);
                    }
                }
            }
            out
        }
    };
    let mut packages: Vec<PathBuf> = all.iter().filter(|p| p.file_name().is_some_and(|n| n == FILE)).filter_map(|p| p.parent().map(|d| d.to_path_buf())).collect();
    packages.sort();
    let mut scripts: Vec<PathBuf> = all
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "strata"))
        .filter(|p| !p.ancestors().skip(1).take_while(|d| d.starts_with(top)).any(|d| d.join(FILE).is_file()))
        .collect();
    scripts.sort();
    Ok((packages, scripts))
}

/// Exports that name nothing: each must be a module of the package, or one
/// of its `pub` definitions.
pub fn bad_exports(pkg: &Package, p: &Program) -> Vec<String> {
    let mut out = Vec::new();
    for e in pkg.exports.iter().flatten() {
        if p.decls.iter().any(|d| d.module == *e) {
            continue;
        }
        match p.decls.iter().find(|d| d.qname() == *e) {
            Some(d) if d.public => {}
            Some(_) => out.push(format!("exports `{e}`, which isn't `pub`")),
            None => out.push(format!("exports `{e}`, which is neither a module nor a definition of this package")),
        }
    }
    out
}
