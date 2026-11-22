//! Packages. A folder with a `strata.toml` is a package; its `.strata` files
//! (binary module files) are its modules, named after their path inside it (`shop/orders.strata`
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
//! [verify]                               # checked by `strata verify`
//! proven = ["core"]                      # modules whose every check must be proven
//! terminates = ["*"]                     # ... whose functions must terminate (or be marked)
//! no_recursion = ["core"]                # ... where no function may be recursive
//!
//! [run]
//! mode = true                            # `main` and tests may ask for `RunMode` (std.process)
//!
//! [native]                               # C for `extern c` (compiled code only)
//! sources = ["c/distance.c"]             # the package's C files, compiled with it
//! libs = ["sqlite3"]                     # libraries to link (`-lsqlite3`)
//! ```

use crate::ast::Program;
use std::path::{Path, PathBuf};

pub const FILE: &str = "strata.toml";

pub struct Package {
    pub dir: PathBuf,
    pub name: String,
    /// `None`: every `pub` definition is visible to other packages
    pub exports: Option<Vec<String>>,
    pub verify: Policy,
    /// each change made through strata is also a git commit (default: yes)
    pub auto_commit: bool,
    /// `[json]`: how `std.json` reads JSON in this package
    pub json: JsonRules,
    /// `[native]`: the C that `extern c` declarations are linked to
    pub native: Native,
    /// `[ui]`: the viewer's settings
    pub ui: UiRules,
}

/// `[run]`: `mode = true` lets `main` and tests ask how the program is run
/// (`RunMode`, std.process). By default they can't tell.
#[derive(serde::Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct RunRules {
    #[serde(default)]
    pub mode: bool,
}

/// `[native]`: C files of the package (paths relative to its folder), and
/// libraries to link, for the `extern c` declarations of its modules.
#[derive(serde::Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Native {
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub libs: Vec<String>,
}

impl Native {
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty() && self.libs.is_empty()
    }
}

/// The package being worked on's folder and `[native]` section, for compiled
/// code (the JIT links them in as `strata build` does).
static NATIVE: std::sync::OnceLock<(PathBuf, Native)> = std::sync::OnceLock::new();

impl Native {
    /// What clang is given to link it in: the package's C files and `-l` libraries.
    pub fn clang_args(&self, dir: &Path) -> Vec<String> {
        let mut out: Vec<String> = self.sources.iter().map(|s| dir.join(s).to_string_lossy().to_string()).collect();
        out.extend(self.libs.iter().map(|l| format!("-l{l}")));
        out
    }
}

/// The C to link the program being worked on with: its package's folder and
/// `[native]` section (none outside a package).
pub fn native() -> Option<&'static (PathBuf, Native)> {
    NATIVE.get().filter(|(_, n)| !n.is_empty())
}

/// `[json]`: `unknown_keys = "error"` makes `decode` refuse keys a record
/// doesn't have (by default they are ignored); `integers = "proven"` makes
/// every integer field have to be shown to fit a JSON number exactly (by
/// default, `encode` returns an error for one that doesn't).
#[derive(serde::Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct JsonRules {
    #[serde(default)]
    pub unknown_keys: Option<String>,
    #[serde(default)]
    pub integers: Option<String>,
}

/// The settings of the package a command works on (the first one loaded),
/// for compile-time code: `std.typing.setting("json.unknown_keys")`.
static SETTINGS: std::sync::OnceLock<Vec<(String, String)>> = std::sync::OnceLock::new();

/// A setting of the package being worked on, by `section.key`.
pub fn setting(key: &str) -> Option<String> {
    SETTINGS.get()?.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

/// `[verify]`: what `strata verify` requires, each as a list of modules
/// (`"*"` for all of them, `"main"` for the root module, a module covers
/// the modules below it). Recursion is allowed unless `no_recursion` says.
#[derive(serde::Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    #[serde(default)]
    pub proven: Vec<String>,
    #[serde(default)]
    pub terminates: Vec<String>,
    #[serde(default)]
    pub no_recursion: Vec<String>,
}

impl Policy {
    /// Does a rule listing `modules` apply to a definition in `module`?
    pub fn covers(modules: &[String], module: &str) -> bool {
        modules.iter().any(|m| m == "*" || (m == "main" && module.is_empty()) || module == m || module.starts_with(&format!("{m}.")))
    }

    fn rules(&self) -> [(&'static str, &Vec<String>); 3] {
        [("proven", &self.proven), ("terminates", &self.terminates), ("no_recursion", &self.no_recursion)]
    }
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
    verify: Policy,
    #[serde(default)]
    auto_commit: Option<bool>,
    #[serde(default)]
    json: JsonRules,
    #[serde(default)]
    native: Native,
    #[serde(default)]
    run: RunRules,
    #[serde(default)]
    ui: UiRules,
}

/// `[ui]`: the viewer's settings (`strata ui`).
#[derive(serde::Deserialize, Clone, Default, Debug)]
#[serde(deny_unknown_fields)]
pub struct UiRules {
    /// the command that runs an agent over ACP, in the Agents mode
    pub agent: Option<String>,
    /// new agent sessions run their tools without asking
    pub auto_allow: Option<bool>,
}

pub fn load(dir: &Path) -> Result<Package, String> {
    let path = dir.join(FILE);
    let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let raw: Raw = toml::from_str(&src).map_err(|e| format!("{}: {e}", path.display()))?;
    if let Some(k) = &raw.json.unknown_keys {
        if k != "error" && k != "ignore" {
            return Err(format!("{}: [json] unknown_keys is \"error\" or \"ignore\", not {k:?}", path.display()));
        }
    }
    if let Some(k) = &raw.json.integers {
        if k != "checked" && k != "proven" {
            return Err(format!("{}: [json] integers is \"checked\" or \"proven\", not {k:?}", path.display()));
        }
    }
    let mut settings = vec![];
    if let Some(k) = &raw.json.unknown_keys {
        settings.push(("json.unknown_keys".to_string(), k.clone()));
    }
    if let Some(k) = &raw.json.integers {
        settings.push(("json.integers".to_string(), k.clone()));
    }
    if raw.run.mode {
        settings.push(("run.mode".to_string(), "true".to_string()));
    }
    let _ = SETTINGS.set(settings);
    for src in &raw.native.sources {
        if !dir.join(src).is_file() {
            return Err(format!("{}: [native] sources: no file {src:?} in the package", path.display()));
        }
    }
    let _ = NATIVE.set((dir.to_path_buf(), raw.native.clone()));
    Ok(Package { dir: dir.to_path_buf(), name: raw.name, exports: raw.exports, verify: raw.verify, auto_commit: raw.auto_commit.unwrap_or(true), json: raw.json, native: raw.native, ui: raw.ui })
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

/// A program's module files, relative to its folder, with their bytes: a
/// package's (without those of packages nested in it), or a single file as `""`.
pub fn files(path: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    if !path.is_dir() {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        return Ok(vec![(PathBuf::new(), bytes)]);
    }
    let mut files = Vec::new();
    for p in walk(path, EXT)? {
        let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        files.push((p.strip_prefix(path).unwrap().to_path_buf(), bytes));
    }
    Ok(files)
}

/// The extension of Strata files: binary module files (docs/binary-format.md).
pub const EXT: &str = "strata";

/// The files with extension `ext` in a package folder, sorted (not those of
/// packages nested in it, nor hidden folders).
fn walk(path: &Path, ext: &str) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let mut todo = vec![path.to_path_buf()];
    while let Some(dir) = todo.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let p = entry.map_err(|e| e.to_string())?.path();
            if p.is_dir() {
                if !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) && !p.join(FILE).is_file() {
                    todo.push(p);
                }
            } else if p.extension().is_some_and(|e| e == ext) {
                files.push(p);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// The program in a folder (each `.strata` file a module) or a single file.
pub fn program(path: &Path) -> Result<Program, String> {
    // the package's settings, for compile-time code
    find(if path.is_dir() { path } else { path.parent().unwrap_or(Path::new(".")) })?;
    if !path.is_dir() {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        return decode_file(path, &bytes);
    }
    let mut decls = Vec::new();
    for (rel, bytes) in files(path)? {
        decls.extend(decode_file(&path.join(rel), &bytes)?.decls);
    }
    Ok(Program { decls })
}

/// Decode a program's files, as `files` gives them (a bundle, or module files).
pub fn decode(files: Vec<(PathBuf, Vec<u8>)>) -> Result<Program, String> {
    let mut decls = Vec::new();
    for (rel, bytes) in files {
        decls.extend(decode_file(&rel, &bytes)?.decls);
    }
    Ok(Program { decls })
}

/// One file's definitions, as stored (references qualified: shown as written
/// by `resolve::written_form`, given the other modules they use): a module
/// file or a bundle. Text is refused: it only goes in through an edit
/// (`strata edit`, `strata add`).
pub fn decode_file(rel: &Path, bytes: &[u8]) -> Result<Program, String> {
    if !crate::binary::is_binary(bytes) {
        return Err(format!("{}: not a Strata file (code is stored in binary: text goes in through `strata edit` or `strata add`)", rel.display()));
    }
    crate::binary::read(bytes).map(|img| img.program).map_err(|e| format!("{}: {e}", rel.display()))
}

/// What is under `top`: package folders, and `.strata` files outside every
/// package (scripts). In a git repository, ignored files are left out.
pub fn discover(top: &Path) -> Result<(Vec<PathBuf>, Vec<PathBuf>), String> {
    let listed = crate::git::run(Some(top), &["ls-files", "--cached", "--others", "--exclude-standard", "--", "*.strata", "*strata.toml"]).ok();
    let all: Vec<PathBuf> = match listed {
        Some(o) => o.lines().map(|l| top.join(l)).filter(|p| p.is_file()).collect(),
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
        .filter(|p| p.extension().is_some_and(|e| e == EXT))
        .filter(|p| !p.ancestors().skip(1).take_while(|d| d.starts_with(top)).any(|d| d.join(FILE).is_file()))
        .collect();
    scripts.sort();
    Ok((packages, scripts))
}

/// `[verify]` rules that name a module the package doesn't have.
pub fn bad_policy(pkg: &Package, p: &Program) -> Vec<String> {
    let mut out = Vec::new();
    for (rule, modules) in pkg.verify.rules() {
        for m in modules {
            let exists = m == "*" || m == "main" || p.decls.iter().any(|d| d.module == *m || d.module.starts_with(&format!("{m}.")));
            if !exists {
                out.push(format!("[verify] {rule} names `{m}`, which is not a module of this package"));
            }
        }
    }
    out
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
