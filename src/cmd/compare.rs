//! `--strata` and `--compare` on the commands the compiler written in Strata
//! (the `compiler/` package, built once into the cache) implements: `check`,
//! `lex`, `parse` (and `--stored`), `render`, `run` and `build`. `--strata`
//! prints the Strata implementation's output alone; `--compare` runs both and
//! prints the lines only one of them gives.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::types::CheckReport;

/// The compiler package built, and the std folder beside it.
fn compiler() -> Result<(PathBuf, Option<PathBuf>), String> {
    let dir = compiler_dir().ok_or("no compiler package found (a `compiler/` folder holding strata.toml in this folder or above, or next to the strata executable; or set STRATA_COMPILER to its path)")?;
    Ok((built(&dir)?, std_dir(&dir)))
}

/// The built compiler's stdout for `args`.
fn run_compiler(exe: &Path, args: &[String]) -> Result<String, String> {
    let o = Command::new(exe).args(args).output().map_err(|e| format!("running {}: {e}", exe.display()))?;
    if !o.status.success() {
        return Err(format!("the Strata implementation failed ({}): {}", o.status, String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// The Strata checker's error lines on `file` (a script or a package folder).
fn strata_check(file: &str) -> Result<Vec<String>, String> {
    let (exe, std) = compiler()?;
    let mut args = vec!["check".to_string()];
    if let Some(s) = std {
        args.extend(["--std".to_string(), s.to_string_lossy().into_owned()]);
    }
    args.push(file.to_string());
    // an error's message may go on over indented lines (`available:` overloads)
    let mut errs: Vec<String> = vec![];
    for l in run_compiler(&exe, &args)?.lines() {
        if l.starts_with("error in ") {
            errs.push(l.to_string());
        } else if let (Some(e), true) = (errs.last_mut(), l.starts_with("  ")) {
            e.push('\n');
            e.push_str(l);
        }
    }
    Ok(errs)
}

/// `strata check --strata`: the Strata checker's errors alone. Whether none.
pub fn check_strata(file: &str) -> Result<bool, String> {
    let errs = strata_check(file)?;
    for l in &errs {
        println!("{l}");
    }
    if errs.is_empty() {
        println!("ok: no errors (the Strata checker)");
    }
    Ok(errs.is_empty())
}

/// Print the errors only one of the checkers finds on `file`, a script or a
/// package folder (`rep` is the Rust checker's report). Whether they agree.
pub fn compare(file: &str, rep: &CheckReport) -> Result<bool, String> {
    let strata = strata_check(file)?;
    let rust: Vec<String> = rep.errors.iter().map(|e| format!("error in {}: {}", e.decl, e.msg)).collect();
    Ok(report(rust, strata, "errors"))
}

/// The lines only in `rust` and only in `strata` (each matched once, in any
/// order). Whether they agree.
fn report(mut rust: Vec<String>, mut strata: Vec<String>, what: &str) -> bool {
    let n = rust.len();
    let ordered = rust == strata;
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
        if ordered || what == "errors" {
            println!("compare: they agree ({n} {what})");
            return true;
        }
        println!("compare: the same {n} {what}, in a different order");
        return false;
    }
    println!("compare: {} only in rust, {} only in strata ({} agree)", rust.len(), strata.len(), n - rust.len());
    false
}

/// `strata lex|parse|render|run|build --strata|--compare` on a script, or
/// (all but `build`) a package: `lex` and `parse` on each module's text,
/// `render` on each module file, `run` on the package packed into a bundle.
/// `--strata` prints the Strata implementation's output; `--compare` also
/// runs this executable without the flag and compares (for `run` and `build`,
/// the programs' stdout, stderr and exit code).
pub fn command(args: &crate::Args, cmd: &str) -> Result<bool, String> {
    let file = args.val("--file").map(String::from).or_else(|| args.pos.get(1).cloned()).ok_or(format!("{cmd} --strata/--compare: give a script (--file F)"))?;
    let package = Path::new(&file).is_dir();
    if package && cmd == "build" {
        return Err("build --strata/--compare covers a script (one file) for now, not a folder or package".into());
    }
    if cmd == "render" && args.pos.len() > 1 {
        return Err("render --strata/--compare renders a whole script, not chosen definitions".into());
    }
    if cmd == "run" && args.flags.contains_key("--") {
        return Err("run --strata/--compare runs the program with the arguments `one` and `two words` (as tests/eval.rs does), not chosen ones".into());
    }
    let compare = args.flag("--compare");
    let (exe, std) = compiler()?;
    let std_files = || -> Vec<String> {
        let mut out = vec![];
        let mut todo: Vec<PathBuf> = std.iter().cloned().collect();
        while let Some(d) = todo.pop() {
            for p in fs::read_dir(&d).into_iter().flatten().flatten().map(|e| e.path()) {
                if p.is_dir() && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
                    todo.push(p);
                } else if p.extension().is_some_and(|e| e == "strata") {
                    out.push(p.to_string_lossy().into_owned());
                }
            }
        }
        out.sort();
        out
    };
    let scratch = std::env::temp_dir().join(format!("strata-compare-{}", std::process::id()));
    fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    let strata_lines = |out: String| -> Vec<String> { out.lines().filter(|l| !l.starts_with("=== ")).map(String::from).collect() };
    let one = |module: &String, unit: &String| -> Result<(Vec<String>, Vec<String>), String> {
        let (theirs, rust_args): (Vec<String>, Vec<String>) = match cmd {
            "lex" => (vec!["lex".into(), unit.clone()], vec!["lex".into(), unit.clone()]),
            "parse" if args.flag("--stored") => ([vec!["stored".into()], std_files(), vec!["--".into(), unit.clone()]].concat(), vec!["parse".into(), "--stored".into(), unit.clone()]),
            "parse" => (vec!["parse".into(), unit.clone()], vec!["parse".into(), unit.clone()]),
            "render" => (vec!["text".into(), unit.clone()], if module.is_empty() { vec!["--file".into(), file.clone(), "render".into()] } else { vec!["--file".into(), file.clone(), "render".into(), module.clone()] }),
            _ => {
                let prelude = scratch.join("prelude.strata");
                let me = std::env::current_exe().map_err(|e| e.to_string())?;
                let o = Command::new(&me).arg("pack").arg("--prelude").arg(&prelude).output().map_err(|e| e.to_string())?;
                if !o.status.success() {
                    return Err(format!("packing the prelude: {}", String::from_utf8_lossy(&o.stderr).trim()));
                }
                let p = prelude.to_string_lossy().into_owned();
                ([vec!["eval".into(), p], std_files(), vec!["--".into(), unit.clone()]].concat(), ["--file", &file, "run", "--", "one", "two words"].map(String::from).to_vec())
            }
        };
        let mut ours = strata_lines(run_compiler(&exe, &theirs)?);
        if cmd == "run" {
            // `OUTPUT…`, `--- exit N`, `===`
            ours.retain(|l| l != "===");
            if let Some(l) = ours.pop() {
                ours.push(format!("exit: {}", l.trim_start_matches("--- exit ")));
            }
        }
        if cmd == "render" && ours.last().is_some_and(|l| l.is_empty()) {
            ours.pop();
        }
        if !compare {
            return Ok((vec![], ours));
        }
        let o = Command::new(std::env::current_exe().map_err(|e| e.to_string())?).args(&rust_args).output().map_err(|e| e.to_string())?;
        let mut rust: Vec<String> = String::from_utf8_lossy(&o.stdout).lines().map(String::from).collect();
        if cmd == "run" {
            // a fault's message (not the stack under it)
            rust.extend(String::from_utf8_lossy(&o.stderr).lines().filter(|l| !l.trim_start().starts_with("stack:")).map(String::from));
            rust.push(format!("exit: {}", o.status.code().unwrap_or(-1)));
        }
        Ok((rust, ours))
    };
    let result = (|| -> Result<bool, String> {
        match cmd {
            "build" => {
                let runtime = compiler_dir().and_then(|d| d.canonicalize().ok()).and_then(|d| Some(d.parent()?.join("src/llvm/runtime.c"))).filter(|r| r.is_file()).ok_or("build --strata/--compare needs the repository's src/llvm/runtime.c beside compiler/")?;
                let ll = scratch.join("program.ll");
                let ir = run_compiler(&exe, &["codegen".into(), file.clone()])?;
                let missing: Vec<&str> = ir.lines().filter(|l| l.starts_with("error:")).collect();
                if !missing.is_empty() {
                    return Err(format!("compiler/codegen doesn't cover this program yet:\n{}", missing.join("\n")));
                }
                fs::write(&ll, ir).map_err(|e| e.to_string())?;
                let ours = if compare { scratch.join("strata") } else { PathBuf::from(args.val("-o").map(String::from).unwrap_or_else(|| Path::new(&file).file_stem().unwrap().to_string_lossy().into_owned())) };
                let cc = std::env::var("STRATA_CC").unwrap_or_else(|_| "clang".into());
                let o = Command::new(&cc).args(["-O2", "-Wno-override-module", "-o"]).arg(&ours).arg(&ll).arg(&runtime).arg("-lm").output().map_err(|e| format!("running {cc}: {e}"))?;
                if !o.status.success() {
                    return Err(format!("{cc} refused compiler/codegen's IR: {}", String::from_utf8_lossy(&o.stderr).trim()));
                }
                if !compare {
                    println!("built {} (compiler/codegen)", ours.display());
                    return Ok(true);
                }
                let rust = scratch.join("rust");
                let o = Command::new(std::env::current_exe().map_err(|e| e.to_string())?).arg("build").arg(&file).arg("-o").arg(&rust).output().map_err(|e| e.to_string())?;
                if !o.status.success() {
                    return Err(format!("the Rust build failed: {}", String::from_utf8_lossy(&o.stderr).trim()));
                }
                let outcome = |exe: &Path| -> Result<Vec<String>, String> {
                    let o = Command::new(exe).output().map_err(|e| e.to_string())?;
                    let mut l: Vec<String> = String::from_utf8_lossy(&o.stdout).lines().map(|l| format!("stdout: {l}")).collect();
                    l.extend(String::from_utf8_lossy(&o.stderr).lines().map(|l| format!("stderr: {l}")));
                    l.push(format!("exit: {:?}", o.status.code()));
                    Ok(l)
                };
                Ok(report(outcome(&rust)?, outcome(&ours)?, "lines"))
            }
            _ => {
                // a package: each module file on its own (`lex`, `parse`,
                // `render`, lines prefixed with the module's name), or the
                // whole program packed into a bundle (`run`)
                let mut units: Vec<(String, String)> = vec![];
                if !package {
                    units.push((String::new(), file.clone()));
                } else if cmd == "run" {
                    let bundle = scratch.join("program.strata");
                    let o = Command::new(std::env::current_exe().map_err(|e| e.to_string())?).args(["--file", &file, "pack"]).arg(&bundle).output().map_err(|e| e.to_string())?;
                    if !o.status.success() {
                        return Err(format!("packing {file}: {}", String::from_utf8_lossy(&o.stderr).trim()));
                    }
                    units.push((String::new(), bundle.to_string_lossy().into_owned()));
                } else {
                    let mut found = vec![];
                    sources(Path::new(&file), Path::new(&file), &mut found);
                    found.retain(|(rel, _)| rel.ends_with(".strata"));
                    found.sort();
                    for (rel, path) in found {
                        let module = rel.trim_end_matches(".strata").replace('/', ".");
                        if cmd == "render" {
                            units.push((module, path.to_string_lossy().into_owned()));
                            continue;
                        }
                        // `lex` and `parse` read text: the module's, as `render` writes it
                        let o = Command::new(std::env::current_exe().map_err(|e| e.to_string())?).args(["--file", &file, "render", &module]).output().map_err(|e| e.to_string())?;
                        if !o.status.success() {
                            return Err(format!("rendering {module}: {}", String::from_utf8_lossy(&o.stderr).trim()));
                        }
                        let text = scratch.join(format!("{module}.strata"));
                        fs::write(&text, &o.stdout).map_err(|e| e.to_string())?;
                        units.push((module, text.to_string_lossy().into_owned()));
                    }
                }
                let (mut all_rust, mut all_ours) = (vec![], vec![]);
                let mut code = None;
                for (module, unit) in &units {
                    let (rust, mut ours) = one(module, unit)?;
                    if cmd == "run" && !compare {
                        code = ours.pop();
                    }
                    let tag = |l: String| if module.is_empty() { l } else { format!("{module}: {l}") };
                    all_rust.extend(rust.into_iter().map(tag));
                    all_ours.extend(ours.into_iter().map(tag));
                }
                if !compare {
                    for l in &all_ours {
                        println!("{l}");
                    }
                    return Ok(code.is_none_or(|c| c == "exit: 0"));
                }
                Ok(report(all_rust, all_ours, "lines"))
            }
        }
    })();
    let _ = fs::remove_dir_all(&scratch);
    result
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
    eprintln!("building the Strata compiler ({}) into {} ...", dir.display(), out.display());
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
