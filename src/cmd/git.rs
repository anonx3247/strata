//! Commands for git: the merge driver, sync, and what CI requires.

use crate::*;

/// `strata merge-file`.
pub fn merge_file(args: &Args) -> Result<bool, String> {
    let (Some(o), Some(a), Some(b)) = (args.pos.get(1), args.pos.get(2), args.pos.get(3)) else {
        return Err("usage: strata merge-file BASE OURS THEIRS [PATH]  (git: `strata merge-file %O %A %B %P`)".into());
    };
    let path = args.pos.get(4).unwrap_or(a);
    let read = |f: &str| std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"));
    let (base, ours, theirs) = (read(o)?, read(a)?, read(b)?);
    match merge::merge(&base, &ours, &theirs) {
        Ok(m) => {
            std::fs::write(a, &m.text).map_err(|e| format!("{a}: {e}"))?;
            let mut ok = m.conflicts.is_empty();
            for n in &m.notes {
                eprintln!("strata: {path}: {n}");
            }
            for c in &m.conflicts {
                eprintln!("strata: {path}: conflict in {c}");
            }
            // under `strata sync`, the check waits until every file is merged and the
            // logged renames are followed: while git merges, the other files are still ours
            let in_sync = std::env::var_os("STRATA_MERGE_BASE").is_some();
            if ok && !in_sync {
                match merge::new_errors(path, &m.text, &ours, &theirs) {
                    Ok(errs) if !errs.is_empty() => {
                        ok = false;
                        eprintln!("strata: {path}: merged definition by definition, but the result doesn't check:");
                        for e in errs {
                            eprintln!("    {e}");
                        }
                    }
                    Ok(_) => {}
                    Err(e) => eprintln!("strata: {path}: not type-checked: {e}"),
                }
            }
            Ok(ok)
        }
        Err(e) => {
            eprintln!("strata: {path}: {e}; merging it as text");
            let (text, clean) = merge::text_merge(&base, &ours, &theirs)?;
            std::fs::write(a, text).map_err(|e| format!("{a}: {e}"))?;
            Ok(clean)
        }
    }
}

/// `strata setup-git`.
pub fn setup_git() -> Result<bool, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let driver = format!("'{}' merge-file %O %A %B %P", exe.display());
    crate::git::run(None, &["config", "merge.strata.name", "Strata: merge each definition separately, then type-check"])?;
    crate::git::run(None, &["config", "merge.strata.driver", &driver])?;
    println!("git will merge .strata files with: {driver}");
    let top = crate::git::run(None, &["rev-parse", "--show-toplevel"])?;
    let attrs = std::fs::read_to_string(std::path::Path::new(top.trim()).join(".gitattributes")).unwrap_or_default();
    if !attrs.lines().any(|l| l.starts_with("*.strata") && l.split_whitespace().any(|w| w == "merge=strata")) {
        println!("add `*.strata merge=strata` to .gitattributes (and commit it) so git uses it");
    }
    Ok(true)
}

/// `strata sync`.
pub fn sync(args: &Args) -> Result<bool, String> {
    let branch = args.pos.get(1).map(|s| s.as_str()).unwrap_or("main");
    if !crate::git::run(None, &["status", "--porcelain", "--untracked-files=no"])?.trim().is_empty() {
        return Err("there are uncommitted changes; commit them first".into());
    }
    let target = if crate::git::run(None, &["remote", "get-url", "origin"]).is_ok() {
        crate::git::run(None, &["fetch", "origin", branch])?;
        format!("origin/{branch}")
    } else {
        branch.to_string()
    };
    crate::git::run(None, &["rev-parse", "--verify", &format!("{target}^{{commit}}")]).map_err(|_| format!("no branch `{target}`"))?;
    let is_ancestor = |a: &str, b: &str| crate::git::ok(None, &["merge-base", "--is-ancestor", a, b]);
    if is_ancestor(&target, "HEAD") {
        println!("already up to date with {target}");
        return Ok(true);
    }
    if is_ancestor("HEAD", &target) {
        crate::git::run(None, &["merge", "--ff-only", &target])?;
        println!("fast-forwarded to {target}");
        return ci(false);
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let attrs = crate::git::run(None, &["rev-parse", "--git-path", "strata-sync-attributes"])?.trim().to_string();
    std::fs::write(&attrs, "*.strata merge=strata\nstrata.log merge=union\n").map_err(|e| format!("{attrs}: {e}"))?;
    let base = crate::git::run(None, &["merge-base", "HEAD", &target])?.trim().to_string();
    let status = Command::new("git")
        .args(["-c", "merge.strata.name=strata", "-c"])
        .arg(format!("merge.strata.driver='{}' merge-file %O %A %B %P", exe.display()))
        .args(["-c", &format!("core.attributesFile={attrs}"), "merge", "--no-ff", "--no-commit", &target])
        // the driver reads the renames both sides logged since the merge base
        .env("STRATA_MERGE_BASE", &base)
        .env("STRATA_MERGE_THEIRS", &target)
        .status()
        .map_err(|e| format!("git: {e}"));
    let _ = std::fs::remove_file(&attrs);
    status?;
    let conflicted = crate::git::run(None, &["diff", "--name-only", "--diff-filter=U"])?;
    if !conflicted.trim().is_empty() {
        eprintln!("\nconflicts with {target} in:");
        for f in conflicted.lines() {
            eprintln!("  {f}");
        }
        eprintln!("resolve them, run `strata ci`, then `git commit` (or `git merge --abort`)");
        return Ok(false);
    }
    // a rename on one side reaches the other side's new calls, in any file
    let top = std::path::PathBuf::from(crate::git::run(None, &["rev-parse", "--show-toplevel"])?.trim());
    for note in merge::follow_renames(&top, &base, &target)? {
        println!("{note}");
    }
    if !ci(false)? {
        eprintln!("\nthe merge with {target} doesn't check, so it is staged but not committed:");
        eprintln!("fix it, run `strata ci`, then `git commit` (or `git merge --abort`)");
        return Ok(false);
    }
    crate::git::run(None, &["commit", "--no-edit"])?;
    println!("merged {target}");
    Ok(true)
}

/// What CI requires, over the whole repository (or the current folder outside
/// git): every package and every script checks, every package's exports name
/// something, and every `.strata` file is in canonical format (what `render`
/// prints). `fix` rewrites the files that aren't.
pub fn ci(fix: bool) -> Result<bool, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let top = crate::git::run(None, &["rev-parse", "--show-toplevel"]).map(|t| std::path::PathBuf::from(t.trim())).unwrap_or(cwd);
    let top = std::fs::canonicalize(&top).unwrap_or(top);
    let (packages, scripts) = package::discover(&top)?;
    if packages.is_empty() && scripts.is_empty() {
        return Err(format!("no .strata files under {}", top.display()));
    }
    let mut ok = true;
    for path in packages.iter().chain(&scripts) {
        let shown = path.strip_prefix(&top).unwrap_or(path).display().to_string();
        let pkg = if path.is_dir() { Some(package::load(path)?) } else { None };
        let files = package::files(path)?;
        let mut parsed = true;
        for (rel, src) in &files {
            let file = if rel.as_os_str().is_empty() { shown.clone() } else { format!("{shown}/{}", rel.display()) };
            match parser::parse_module_file(src, "") {
                Err(e) => {
                    ok = false;
                    parsed = false;
                    println!("✗ {file}: doesn't parse: {e}");
                }
                Ok(p) if printer::program(&p) != *src => {
                    if fix {
                        std::fs::write(top.join(&file), printer::program(&p)).map_err(|e| format!("{file}: {e}"))?;
                        println!("  {file}: rewritten in canonical format");
                    } else {
                        ok = false;
                        println!("✗ {file}: not in canonical format (`strata ci --fix` rewrites it)");
                    }
                }
                Ok(_) => {}
            }
        }
        if !parsed {
            continue;
        }
        let p = package::program(path)?;
        let rep = full_check(&p);
        let mut errors: Vec<String> = rep.errors.iter().map(|e| format!("error in {}: {}", e.decl, e.msg)).collect();
        // holes of an unfinished change are not to be merged
        errors.extend(rep.holes.iter().filter(|h| h.name.starts_with(pending::PREFIX)).map(|h| {
            format!("?{} in {} belongs to an unfinished change: fill it and run `strata done`, or `strata abort`", h.name, h.decl)
        }));
        if let Some(pkg) = &pkg {
            errors.extend(package::bad_exports(pkg, &p).into_iter().chain(package::bad_policy(pkg, &p)).map(|e| format!("{}: {e}", package::FILE)));
        }
        let label = match &pkg {
            Some(pkg) => format!("package {} ({shown})", pkg.name),
            None => shown.clone(),
        };
        if errors.is_empty() {
            println!("✓ {label}: {} definitions check ({} open holes)", p.decls.len(), rep.holes.len());
        } else {
            ok = false;
            println!("✗ {label}:");
            for e in errors {
                println!("    {e}");
            }
        }
    }
    Ok(ok)
}
