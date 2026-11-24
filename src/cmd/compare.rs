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
    // it reads `core/` from the folder it runs in, the repository's root: the
    // files it is given, made absolute
    let root = compiler_dir().and_then(|d| d.canonicalize().ok()).and_then(|d| Some(d.parent()?.to_path_buf()));
    // (not the program's own arguments, after `--program`: they are its to read)
    let own = args.iter().position(|a| a == "--program").unwrap_or(args.len());
    let args: Vec<String> = args
        .iter()
        .enumerate()
        .map(|(k, a)| match Path::new(a).canonicalize() {
            // (the first is the command's name)
            Ok(abs) if k > 0 && k < own && root.is_some() => abs.to_string_lossy().into_owned(),
            _ => a.clone(),
        })
        .collect();
    let mut c = Command::new(exe);
    // whether what it prints for us shows colors (std.term's rule): the
    // program run by compiler/'s evaluator writes to our terminal
    c.env("STRATA_COLORS", colors());
    if let Some(r) = &root {
        c.current_dir(r);
    }
    let o = c.args(&args).output().map_err(|e| format!("running {}: {e}", exe.display()))?;
    if !o.status.success() {
        return Err(format!("the Strata implementation failed ({}): {}", o.status, String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// `out,err`: which of standard output and error show colors (a terminal,
/// unless `NO_COLOR` is set or `TERM` is `dumb`; `FORCE_COLOR` forces them).
fn colors() -> String {
    use std::io::IsTerminal;
    let set = |v: &str| std::env::var(v).is_ok_and(|x| !x.is_empty());
    let on = |tty: bool| set("FORCE_COLOR") || (tty && !set("NO_COLOR") && std::env::var("TERM").map_or(true, |t| t != "dumb"));
    let mut out = vec![];
    if on(std::io::stdout().is_terminal()) {
        out.push("out");
    }
    if on(std::io::stderr().is_terminal()) {
        out.push("err");
    }
    out.join(",")
}

/// std precompiled by compiler/codegen (plan item C21): its functions that
/// compile once for every program (`compiler codegen --library`), as an
/// object `build` links statically and a shared library the compiler's JIT
/// loads, with their names (`--precompiled`). Kept beside the built
/// compiler, whose cache key covers std's files. `None` with
/// `STRATA_NO_STD_LIBRARY`, or when it can't be made (each program then
/// compiles the std code it uses).
struct StdLibrary {
    names: PathBuf,
    object: PathBuf,
    shared: PathBuf,
}

fn std_library(exe: &Path) -> Option<StdLibrary> {
    if std::env::var_os("STRATA_NO_STD_LIBRARY").is_some() {
        return None;
    }
    let dir = exe.parent()?.join("std");
    let lib = StdLibrary { names: dir.join("names"), object: dir.join("std.o"), shared: dir.join(if cfg!(target_os = "macos") { "libstd.dylib" } else { "libstd.so" }) };
    if lib.names.is_file() {
        return Some(lib);
    }
    let made = (|| -> Result<(), String> {
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let ir = run_compiler(exe, &["codegen".into(), "--library".into()])?;
        if ir.lines().any(|l| l.starts_with("error")) {
            return Err(ir.lines().filter(|l| l.starts_with("error")).collect::<Vec<_>>().join("\n"));
        }
        // (each step to a file of its own, renamed when done: another
        // process may be making it too)
        let id = std::process::id();
        let ll = dir.join(format!("std.{id}.ll"));
        fs::write(&ll, &ir).map_err(|e| e.to_string())?;
        let cc = std::env::var("STRATA_CC").unwrap_or_else(|_| "clang".into());
        let (obj, shared) = (dir.join(format!("std.{id}.o")), dir.join(format!("libstd.{id}")));
        let mut so: Vec<&str> = vec!["-O2", "-fPIC", "-shared", "-Wno-override-module"];
        if cfg!(target_os = "macos") {
            so.extend(["-undefined", "dynamic_lookup"]);
        }
        for args in [vec!["-O2", "-fPIC", "-Wno-override-module", "-c", "-o"], [so, vec!["-o"]].concat()] {
            let out = if args.contains(&"-c") { &obj } else { &shared };
            let o = Command::new(&cc).args(&args).arg(out).arg(&ll).output().map_err(|e| format!("running {cc}: {e}"))?;
            if !o.status.success() {
                return Err(String::from_utf8_lossy(&o.stderr).into_owned());
            }
        }
        let _ = fs::remove_file(&ll);
        fs::rename(&obj, &lib.object).map_err(|e| e.to_string())?;
        fs::rename(&shared, &lib.shared).map_err(|e| e.to_string())?;
        let names: Vec<&str> = ir.lines().filter_map(|l| l.strip_prefix("; precompiled ")).collect();
        let tmp = dir.join(format!("names.{id}"));
        fs::write(&tmp, names.join("\n") + "\n").map_err(|e| e.to_string())?;
        fs::rename(&tmp, &lib.names).map_err(|e| e.to_string())
    })();
    match made {
        Ok(()) => Some(lib),
        Err(e) => {
            if std::env::var_os("STRATA_DEBUG").is_some() {
                eprintln!("note: std is compiled with each program: {e}");
            }
            None
        }
    }
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
    if cmd == "render" && args.pos.len() > 1 {
        return Err("render --strata/--compare renders a whole script, not chosen definitions".into());
    }
    // `run`: the program's arguments (`-- ARGS`, else `one` and `two words`,
    // as tests/eval.rs gives them) and its name, for `--help`: its
    // package's, or the file's (as `strata run` names it)
    let program_args: Vec<String> = args.flags.get("--").cloned().unwrap_or_else(|| vec!["one".into(), "two words".into()]);
    let program_name = match crate::package::find(Path::new(&file)) {
        Ok(Some(pkg)) => pkg.name,
        _ => Path::new(&file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "main".into()),
    };
    let compare = args.flag("--compare");
    let (exe, std) = compiler()?;
    // `run --strata --jit`: compiled by compiler/codegen and run by the
    // compiler's JIT (`compiler jit`), its output and exit code the program's
    if cmd == "run" && args.flag("--jit") && !compare {
        let root = compiler_dir().and_then(|d| d.canonicalize().ok()).and_then(|d| Some(d.parent()?.to_path_buf())).ok_or("no repository around compiler/")?;
        let path = Path::new(&file).canonicalize().map_err(|e| format!("{file}: {e}"))?;
        // (`--trace`: each call and return on stderr, shown by the compiled code)
        let trace: &[&str] = if args.flag("--trace") { &["--trace"] } else { &[] };
        // (std's precompiled code: loaded, not compiled again)
        let mut pre: Vec<std::ffi::OsString> = vec![];
        if let Some(l) = std_library(&exe) {
            pre.extend(["--load".into(), l.shared.into_os_string(), "--precompiled".into(), l.names.into_os_string()]);
        }
        let status = Command::new(&exe).current_dir(root).arg("jit").args(&pre).args(trace).arg(path).arg("--").args(&program_args).status().map_err(|e| e.to_string())?;
        std::process::exit(status.code().unwrap_or(1));
    }
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
    // a package's module files other than `unit` (none for a script)
    let siblings = |unit: &String| -> Vec<String> {
        if !package {
            return vec![];
        }
        let mut found = vec![];
        sources(Path::new(&file), Path::new(&file), &mut found);
        let mut out: Vec<String> = found.into_iter().filter(|(rel, p)| rel.ends_with(".strata") && p.to_string_lossy() != unit.as_str()).map(|(_, p)| p.to_string_lossy().into_owned()).collect();
        out.sort();
        out
    };
    let scratch = std::env::temp_dir().join(format!("strata-compare-{}", std::process::id()));
    fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    let strata_lines = |out: String| -> Vec<String> { out.lines().filter(|l| !l.starts_with("=== ")).map(String::from).collect() };
    let one = |module: &String, unit: &String| -> Result<(Vec<String>, Vec<String>), String> {
        let (theirs, rust_args): (Vec<String>, Vec<String>) = match cmd {
            "lex" => (vec!["lex".into(), unit.clone()], vec!["lex".into(), "--rust".into(), unit.clone()]),
            "parse" if args.flag("--stored") => ([vec!["stored".into()], std_files(), vec!["--".into(), unit.clone()]].concat(), vec!["parse".into(), "--rust".into(), "--stored".into(), unit.clone()]),
            "parse" => (vec!["parse".into(), unit.clone()], vec!["parse".into(), "--rust".into(), unit.clone()]),
            // written back as `strata render` writes it (names as short as
            // the module's imports allow): std's and the package's other
            // modules give the names
            "render" => ([vec!["written".into()], std_files(), siblings(unit), vec!["--".into(), unit.clone()]].concat(), if module.is_empty() { vec!["--file".into(), file.clone(), "render".into(), "--rust".into()] } else { vec!["--file".into(), file.clone(), "render".into(), module.clone(), "--rust".into()] }),
            _ => {
                let prelude = scratch.join("prelude.strata");
                let me = std::env::current_exe().map_err(|e| e.to_string())?;
                let o = Command::new(&me).arg("pack").arg("--prelude").arg(&prelude).output().map_err(|e| e.to_string())?;
                if !o.status.success() {
                    return Err(format!("packing the prelude: {}", String::from_utf8_lossy(&o.stderr).trim()));
                }
                let p = prelude.to_string_lossy().into_owned();
                (
                    [vec!["eval".into(), p], std_files(), vec!["--".into(), unit.clone(), "--program".into(), program_name.clone()], program_args.clone()].concat(),
                    [["--file", &file, "run", "--rust", "--"].map(String::from).to_vec(), program_args.clone()].concat(),
                )
            }
        };
        let mut ours = strata_lines(run_compiler(&exe, &theirs)?);
        if cmd == "run" {
            // `OUTPUT…`, `--- stderr`, `ERRORS…`, `--- exit N`, `===`; run
            // on its own, the errors go to stderr (marked with a NUL here)
            ours.retain(|l| l != "===");
            if let Some(at) = ours.iter().position(|l| l == "--- stderr") {
                ours.remove(at);
                if !compare {
                    let last = ours.len().saturating_sub(1);
                    for l in &mut ours[at..last] {
                        l.insert(0, '\0');
                    }
                }
            }
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
        if cmd == "render" && !o.status.success() {
            return Err(format!("rendering {}: {}", if module.is_empty() { &file } else { module }, String::from_utf8_lossy(&o.stderr).trim()));
        }
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
            // the tests' IR from `compiler codegen --tests` (`strata.test.N`, each
            // after a line naming it), run as `strata test` runs its own
            "test" => {
                let ir = run_compiler(&exe, &["codegen".into(), "--tests".into(), file.clone()])?;
                let missing: Vec<&str> = ir.lines().filter(|l| l.starts_with("error:")).collect();
                if !missing.is_empty() {
                    return Err(format!("compiler/codegen doesn't cover these tests yet:\n{}", missing.join("\n")));
                }
                let names: Vec<String> = ir.lines().filter_map(|l| l.strip_prefix("; test ")).map(|l| l.split_once(": ").map(|x| x.1.to_string()).unwrap_or_default()).collect();
                let filter = if args.val("--file").is_some() { args.pos.get(1).cloned().unwrap_or_default() } else { String::new() };
                let which: Vec<usize> = (0..names.len()).filter(|&i| names[i].contains(&filter)).collect();
                let runs = crate::llvm::jit::run_tests(&ir, &which)?;
                let (mut pass, mut fail, mut ours) = (0, 0, vec![]);
                for (&i, r) in which.iter().zip(runs) {
                    match r.fault {
                        None => {
                            pass += 1;
                            ours.push(format!("✓ {}", names[i]));
                        }
                        Some((kind, msg, stack)) => {
                            fail += 1;
                            ours.push(format!("✗ {}", names[i]));
                            ours.push(format!("    {kind}: {msg}"));
                            if !stack.is_empty() {
                                ours.push(format!("    stack: {stack}"));
                            }
                            if !r.printed.is_empty() {
                                ours.push("    printed:".into());
                                ours.extend(r.printed.lines().map(|l| format!("      {l}")));
                            }
                        }
                    }
                }
                ours.push(String::new());
                ours.push(format!("{pass} passed, {fail} failed"));
                if !compare {
                    for l in &ours {
                        println!("{l}");
                    }
                    return Ok(fail == 0);
                }
                let mut rust_args = vec!["--file".to_string(), file.clone(), "test".to_string(), "--rust".to_string()];
                if !filter.is_empty() {
                    rust_args.push(filter);
                }
                let o = Command::new(std::env::current_exe().map_err(|e| e.to_string())?).args(&rust_args).output().map_err(|e| e.to_string())?;
                let rust: Vec<String> = String::from_utf8_lossy(&o.stdout).lines().map(String::from).collect();
                Ok(report(rust, ours, "lines"))
            }
            "build" => {
                let runtime = compiler_dir().and_then(|d| d.canonicalize().ok()).and_then(|d| Some(d.parent()?.join("src/llvm/runtime.c"))).filter(|r| r.is_file()).ok_or("build --strata/--compare needs the repository's src/llvm/runtime.c beside compiler/")?;
                let ll = scratch.join("program.ll");
                // the checks `verify` proved, left out as `strata build` leaves
                // them out (what changed since verified first; `--all-checks`
                // keeps them all, `--no-verify` uses what was proven before)
                let proven = scratch.join("proofs.txt");
                // (verified by `compiler verify`, its record read by `compiler proofs`)
                let facts = if args.flag("--all-checks") {
                    String::new()
                } else {
                    strata_proofs(Path::new(&file), !args.flag("--no-verify"))?.unwrap_or_default()
                };
                fs::write(&proven, facts).map_err(|e| e.to_string())?;
                // std's precompiled code: declared here, linked in (statically)
                let std_lib = std_library(&exe);
                let mut cg: Vec<String> = vec!["codegen".into(), "--proofs".into(), proven.to_string_lossy().into_owned()];
                if let Some(l) = &std_lib {
                    cg.extend(["--precompiled".into(), l.names.to_string_lossy().into_owned()]);
                }
                cg.push(file.clone());
                let ir = run_compiler(&exe, &cg)?;
                let missing: Vec<&str> = ir.lines().filter(|l| l.starts_with("error:")).collect();
                if !missing.is_empty() {
                    return Err(format!("compiler/codegen doesn't cover this program yet:\n{}", missing.join("\n")));
                }
                fs::write(&ll, ir).map_err(|e| e.to_string())?;
                // core.rt, the runtime written in Strata (the allocator),
                // compiled by compiler/codegen too: linked in place of the C
                // runtime's own versions (`STRATA_CORE_RT`), as `build --rust`
                // links Rust's compile of it
                let core_rt_ir = run_compiler(&exe, &["codegen".into(), "--core-rt".into()])?;
                if let Some(e) = core_rt_ir.lines().find(|l| l.starts_with("error")) {
                    return Err(format!("compiler/codegen can't compile core.rt: {e}"));
                }
                let core_rt = scratch.join("core_rt.ll");
                fs::write(&core_rt, core_rt_ir).map_err(|e| e.to_string())?;
                let ours = if compare { scratch.join("strata") } else { PathBuf::from(args.val("-o").map(String::from).unwrap_or_else(|| Path::new(&file).file_stem().unwrap().to_string_lossy().into_owned())) };
                let cc = std::env::var("STRATA_CC").unwrap_or_else(|_| "clang".into());
                // a package's own C (`[native]`), for its `extern c` declarations
                let mut native = crate::package::find(Path::new(&file)).ok().flatten().map(|p| p.native.clang_args(&p.dir)).unwrap_or_default();
                native.retain(|a| a != "-lm");
                let o = Command::new(&cc).args(["-O2", "-Wno-override-module", "-DSTRATA_CORE_RT", "-o"]).arg(&ours).arg(&ll).arg(&core_rt).arg(&runtime).args(std_lib.as_ref().map(|l| l.object.clone())).args(&native).arg("-lm").output().map_err(|e| format!("running {cc}: {e}"))?;
                if !o.status.success() {
                    return Err(format!("{cc} refused compiler/codegen's IR: {}", String::from_utf8_lossy(&o.stderr).trim()));
                }
                if !compare {
                    println!("built {} (compiler/codegen)", ours.display());
                    return Ok(true);
                }
                let rust = scratch.join("rust");
                let o = Command::new(std::env::current_exe().map_err(|e| e.to_string())?).arg("build").arg("--rust").arg(&file).arg("-o").arg(&rust).output().map_err(|e| e.to_string())?;
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
                    // std's modules are named `std.io`, ...
                    let is_std = fs::read_to_string(Path::new(&file).join("strata.toml")).is_ok_and(|t| t.lines().any(|l| l.trim() == "name = \"std\""));
                    for (rel, path) in found {
                        let module = rel.trim_end_matches(".strata").replace('/', ".");
                        let module = if is_std { format!("std.{module}") } else { module };
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
                    // what the evaluator doesn't run yet: Rust's, by default
                    if cmd == "run" && !args.flag("--strata") && all_ours.iter().any(|l| l.contains("isn't run by this evaluator") || l.contains("which this evaluator can't give")) {
                        return Err(NOT_COVERED.into());
                    }
                    for l in &all_ours {
                        match l.strip_prefix('\0') {
                            Some(e) => eprintln!("{e}"),
                            None => println!("{l}"),
                        }
                    }
                    // the program's own exit code
                    if let Some(n) = code.as_deref().and_then(|c| c.strip_prefix("exit: ")).and_then(|n| n.parse::<i32>().ok()).filter(|n| *n != 0) {
                        std::process::exit(n);
                    }
                    return Ok(true);
                }
                Ok(report(all_rust, all_ours, "lines"))
            }
        }
    })();
    let _ = fs::remove_dir_all(&scratch);
    result
}

/// Whether a compiler package is there to run (`lex` and `parse` use it by
/// default).
pub fn available() -> bool {
    compiler_dir().is_some()
}

/// The marker of a run the Strata side can't give (`command` returns it as
/// its error, before printing anything, so Rust's runs instead).
const NOT_COVERED: &str = "not covered by compiler/";

/// Whether `render` as given is the Strata side's by default: a whole
/// script rendered with no view's options. (`run` is compiler/codegen's
/// and the compiler's JIT's: `run::run_program`.)
pub fn covers_by_default(args: &crate::Args, cmd: &str) -> bool {
    if !available() {
        return false;
    }
    match cmd {
        "render" => {
            let file = args.val("--file");
            args.pos.len() == 1
                && file.is_some_and(|f| Path::new(f).is_file())
                && !["--split", "--members", "--level", "--prelude", "--text", "--tree", "--mocks", "--json"].iter().any(|f| args.flags.contains_key(*f))
        }
        _ => false,
    }
}

/// Whether `STRATA_BACKEND=rust` asks for the Rust code generator (as
/// `--rust` does).
pub fn rust_backend() -> bool {
    std::env::var("STRATA_BACKEND").is_ok_and(|v| v == "rust")
}

/// Whether `build` or `test` as given is compiler/codegen's by default: a
/// program or package named (`test`: with `--file`), not the compiler
/// package itself (its build would need itself), without the options only
/// the Rust side has (`--emit-llvm`; `test`'s `--with-deps`, `--jit`,
/// `--interpret`).
pub fn codegen_by_default(args: &crate::Args, cmd: &str) -> bool {
    let Some(dir) = compiler_dir().and_then(|d| d.canonicalize().ok()) else { return false };
    let path = match cmd {
        "build" => args.pos.get(1).map(String::from).or(args.val("--file").map(String::from)),
        _ => args.val("--file").map(String::from),
    };
    let Some(path) = path.and_then(|p| Path::new(&p).canonicalize().ok()) else { return false };
    let own = ["--emit-llvm", "--with-deps", "--jit", "--interpret", "--port"];
    path != dir && !path.starts_with(&dir) && !own.iter().any(|f| args.flags.contains_key(*f)) && (cmd != "build" || args.pos.len() <= 2)
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
/// package's and std's files, and this executable, so a change rebuilds it.
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
    // (by Rust: never compiler/codegen, which this build makes)
    let o = Command::new(&me).arg("build").arg("--rust").arg("--no-verify").arg(dir).arg("-o").arg(&tmp).env("STRATA_BACKEND", "rust").output().map_err(|e| e.to_string())?;
    if !o.status.success() {
        let _ = fs::remove_file(&tmp);
        return Err(format!("--compare: strata build {} failed:\n{}{}", dir.display(), String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)));
    }
    fs::rename(&tmp, &exe).map_err(|e| e.to_string())?;
    Ok(exe)
}

/// The `.strata` files, settings and C (`[native]`: the compiler's JIT is
/// `c/jit.c`) under `dir` (not hidden folders such as the package's
/// cache), with their paths relative to `root`.
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
        } else if name.ends_with(".strata") || name == "strata.toml" || name == "strata.lock" || name.ends_with(".c") || name.ends_with(".h") {
            out.push((p.strip_prefix(root).unwrap_or(&p).to_string_lossy().into_owned(), p));
        }
    }
}

/// `strata verify` (unless `--rust`, `STRATA_BACKEND=rust`, or no compiler
/// package): `compiler verify` on the script or package (`--file`, else the
/// package around the current folder), its report printed and its exit
/// code kept; a selection (`verify NAME`) as `--only`. `--obligations`,
/// `--termination`, `--fuzz` and `--smt` are `compiler smt`, `termination`,
/// `fuzz` and `smt --queries`. `--compare`: also `strata verify --rust
/// --no-race` (the Strata side has no race), and the lines only one of them
/// gives.
pub fn verify(args: &crate::Args) -> Result<bool, String> {
    let file = match args.val("--file") {
        Some(f) => f.to_string(),
        None => {
            let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
            let root = crate::Codebase::find(&cwd).ok_or("no package here (no strata.toml in this folder or above: run `strata init`, or pass --file)")?;
            root.to_string_lossy().into_owned()
        }
    };
    let file = Path::new(&file).canonicalize().map_err(|e| format!("{file}: {e}"))?.to_string_lossy().into_owned();
    let selection = args.pos.get(1).cloned();
    let (exe, _) = compiler()?;
    let mut ours_args: Vec<String> = vec![];
    let mut rust_args: Vec<String> = vec!["--file".into(), file.clone(), "verify".into(), "--rust".into(), "--no-race".into()];
    let mode = ["--obligations", "--termination", "--fuzz", "--smt"].into_iter().find(|m| args.flag(m));
    match mode {
        Some("--obligations") => ours_args.push("smt".into()),
        Some("--termination") => ours_args.push("termination".into()),
        Some("--smt") => ours_args.extend(["smt".into(), "--queries".into()]),
        Some(_) => ours_args.push("fuzz".into()),
        None => ours_args.push("verify".into()),
    }
    if let Some(m) = mode {
        rust_args.push(m.into());
    }
    let opts: &[&str] = match mode {
        None => &["--cases", "--seed", "--cross-check", "--require"],
        Some("--fuzz") => &["--cases", "--seed"],
        _ => &[],
    };
    for v in opts {
        if let Some(x) = args.val(v) {
            ours_args.extend([v.to_string(), x.to_string()]);
            rust_args.extend([v.to_string(), x.to_string()]);
        }
    }
    if mode.is_none() {
        for f in ["--checks", "--no-cache"] {
            if args.flag(f) {
                ours_args.push(f.into());
                rust_args.push(f.into());
            }
        }
        // std's precompiled code, loaded into the compiler's JIT: what the
        // drivers call of std isn't compiled again (std.web's takes minutes)
        if let Some(l) = std_library(&exe) {
            ours_args.extend(["--load".into(), l.shared.to_string_lossy().into_owned(), "--precompiled".into(), l.names.to_string_lossy().into_owned()]);
        }
    }
    if let Some(sel) = &selection {
        ours_args.extend(["--only".into(), sel.clone()]);
        rust_args.push(sel.clone());
    }
    ours_args.push(file.clone());
    if !args.flag("--compare") {
        // (exit 1: a counterexample or a rule broken; 2: a mistake in the
        // arguments, already said)
        return match compiler_status(&exe, &ours_args)? {
            0 => Ok(true),
            1 => Ok(false),
            code => std::process::exit(code),
        };
    }
    let ours: Vec<String> = compiler_output(&exe, &ours_args)?.lines().map(String::from).collect();
    let o = Command::new(std::env::current_exe().map_err(|e| e.to_string())?).args(&rust_args).output().map_err(|e| e.to_string())?;
    let rust: Vec<String> = String::from_utf8_lossy(&o.stdout).lines().map(String::from).collect();
    // (whether a verdict was kept from an earlier run differs between the two caches)
    let plain = |ls: Vec<String>| -> Vec<String> { ls.into_iter().map(|l| l.replace(" (cached)", "")).collect() };
    Ok(report(plain(rust), plain(ours), "lines"))
}

/// The built compiler run with `args` in the repository's root (it reads
/// `core/` and `std/` there), printing to our stdout and stderr: its exit
/// code.
fn compiler_status(exe: &Path, args: &[String]) -> Result<i32, String> {
    let root = compiler_dir().and_then(|d| d.canonicalize().ok()).and_then(|d| Some(d.parent()?.to_path_buf())).ok_or("no repository around compiler/")?;
    let status = Command::new(exe).env("STRATA_COLORS", colors()).current_dir(root).args(args).status().map_err(|e| format!("running {}: {e}", exe.display()))?;
    Ok(status.code().unwrap_or(1))
}

/// The built compiler's stdout for `args`, whatever its exit code (its
/// stderr passed on).
fn compiler_output(exe: &Path, args: &[String]) -> Result<String, String> {
    let root = compiler_dir().and_then(|d| d.canonicalize().ok()).and_then(|d| Some(d.parent()?.to_path_buf())).ok_or("no repository around compiler/")?;
    let o = Command::new(exe).current_dir(root).args(args).stderr(std::process::Stdio::inherit()).output().map_err(|e| format!("running {}: {e}", exe.display()))?;
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// What the last `verify` proved of the script or package `file` (its code
/// as it is now), as `compiler codegen --proofs` reads it (`compiler
/// proofs`): verified first when `verify` is set (`build`: what changed
/// since, `compiler verify --quiet`, its line on stderr). `None` when the
/// compiler package isn't there or no SMT solver is.
pub fn strata_proofs(file: &Path, verify: bool) -> Result<Option<String>, String> {
    if !available() {
        return Ok(None);
    }
    let (exe, _) = compiler()?;
    let file = file.canonicalize().map_err(|e| format!("{}: {e}", file.display()))?.to_string_lossy().into_owned();
    if verify {
        if crate::smt::solver().is_none() {
            eprintln!("note: no SMT solver (install z3): nothing is proven, so every check stays");
            return Ok(None);
        }
        let mut a: Vec<String> = vec!["verify".into(), "--quiet".into()];
        if let Some(l) = std_library(&exe) {
            a.extend(["--load".into(), l.shared.to_string_lossy().into_owned(), "--precompiled".into(), l.names.to_string_lossy().into_owned()]);
        }
        a.push(file.clone());
        // (a counterexample doesn't stop the build: its checks stay)
        compiler_status(&exe, &a)?;
    }
    compiler_output(&exe, &["proofs".into(), file]).map(Some)
}

/// `strata_proofs`'s text as `llvm.rs` takes it: each function's checks
/// proven and left open (the entry facts are its own to find).
pub fn proofs_of_text(text: &str) -> crate::fx::HashMap<String, crate::llvm::Proofs> {
    let mut out: crate::fx::HashMap<String, crate::llvm::Proofs> = Default::default();
    for l in text.lines() {
        let mut parts = l.splitn(3, '\t');
        let (Some(what), Some(f), Some(check)) = (parts.next(), parts.next(), parts.next()) else { continue };
        let e = out.entry(f.to_string()).or_insert_with(|| crate::llvm::Proofs { proven: vec![], open: vec![] });
        match what {
            "proven" => e.proven.push(check.to_string()),
            "open" => e.open.push(check.to_string()),
            _ => {}
        }
    }
    out
}

/// What `strata run` hands the compiler's JIT besides the program: one
/// function's call (`run f ARGS`): the module file holding the function
/// that makes it (`codegen --call`), and the function called.
pub struct Call {
    pub module_file: Vec<u8>,
    pub fun: String,
}

/// `strata run`'s default: the program (a script or a package's folder,
/// `path`) compiled by compiler/codegen and run by the compiler's JIT
/// (`compiler jit`), named `name`, without the checks `proofs` lists (as
/// `llvm::proofs_text` writes them); or the call `call` made, its result
/// printed. Its exit code. An error starting with `compiler/codegen doesn't
/// cover` when codegen can't compile it yet (nothing ran: the caller runs
/// it with `llvm.rs`).
pub fn jit_run(path: &Path, name: &str, proofs: &str, trace: bool, call: Option<&Call>, program_args: &[String]) -> Result<i32, String> {
    let (exe, _) = compiler()?;
    let root = compiler_dir().and_then(|d| d.canonicalize().ok()).and_then(|d| Some(d.parent()?.to_path_buf())).ok_or("no repository around compiler/")?;
    let path = path.canonicalize().map_err(|e| format!("{}: {e}", path.display()))?;
    let scratch = std::env::temp_dir().join(format!("strata-jit-{}", std::process::id()));
    fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    let errors = scratch.join("errors.txt");
    let proven = scratch.join("proofs.txt");
    fs::write(&proven, proofs).map_err(|e| e.to_string())?;
    let mut c = Command::new(&exe);
    c.current_dir(root).arg("jit");
    // (std's precompiled code: loaded, not compiled again)
    let std_lib = std_library(&exe);
    if let Some(l) = &std_lib {
        c.arg("--load").arg(&l.shared);
    }
    // the package's C (`[native]`), for its `extern c` functions: a shared
    // library loaded too
    let pkg = if path.is_dir() { crate::package::find(&path).ok().flatten() } else { None };
    if let Some(pkg) = pkg.filter(|p| !p.native.is_empty()) {
        let lib = scratch.join(if cfg!(target_os = "macos") { "native.dylib" } else { "native.so" });
        // (an empty file, so libraries alone still make a library)
        let empty = scratch.join("empty.c");
        fs::write(&empty, "").map_err(|e| e.to_string())?;
        let cc = std::env::var("STRATA_CC").unwrap_or_else(|_| "clang".into());
        let o = Command::new(&cc).args(["-shared", "-fPIC", "-O2", "-o"]).arg(&lib).arg(&empty).args(pkg.native.clang_args(&pkg.dir)).output().map_err(|e| format!("can't run `{cc}` to compile the package's C ([native] in strata.toml): {e}"))?;
        if !o.status.success() {
            let _ = fs::remove_dir_all(&scratch);
            return Err(format!("`{cc}` failed on the package's C ([native] in strata.toml):\n{}", String::from_utf8_lossy(&o.stderr).trim_end()));
        }
        c.arg("--load").arg(&lib);
    }
    c.arg("--name").arg(name).arg("--errors").arg(&errors);
    if let Some(call) = call {
        c.arg("--fun").arg(&call.fun);
    }
    c.arg("--proofs").arg(&proven);
    if let Some(l) = &std_lib {
        c.arg("--precompiled").arg(&l.names);
    }
    if trace {
        c.arg("--trace");
    }
    if let Some(call) = call {
        let made = scratch.join("call.strata");
        fs::write(&made, &call.module_file).map_err(|e| e.to_string())?;
        c.arg("--call").arg(made);
    }
    let status = c.arg(&path).arg("--").args(program_args).status().map_err(|e| format!("running {}: {e}", exe.display()));
    let not_covered = fs::read_to_string(&errors).unwrap_or_default();
    let _ = fs::remove_dir_all(&scratch);
    let status = status?;
    if status.code() == Some(3) && !not_covered.trim().is_empty() {
        let missing: Vec<&str> = not_covered.lines().filter(|l| l.starts_with("error:")).collect();
        return Err(format!("compiler/codegen doesn't cover this program yet:\n{}", missing.join("\n")));
    }
    Ok(status.code().unwrap_or(1))
}

/// Whether `path` is the compiler package itself (or inside it), which
/// compiler/codegen doesn't compile: building it would need it built.
pub fn is_compiler(path: &Path) -> bool {
    match (compiler_dir().and_then(|d| d.canonicalize().ok()), path.canonicalize()) {
        (Some(dir), Ok(p)) => p.starts_with(&dir),
        _ => true,
    }
}
