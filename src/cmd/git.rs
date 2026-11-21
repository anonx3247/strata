//! Commands for git: the merge driver, sync, and what CI requires.

use crate::*;

/// `strata merge-file`. The three sides are module files: each is decoded,
/// the definitions are merged (on their text), and the result is written to
/// OURS as a module file. A conflict leaves OURS as text with conflict
/// markers, for a person: `strata resolve` stores it again once fixed.
pub fn merge_file(args: &Args) -> Result<bool, String> {
    let (Some(o), Some(a), Some(b)) = (args.pos.get(1), args.pos.get(2), args.pos.get(3)) else {
        return Err("usage: strata merge-file BASE OURS THEIRS [PATH]  (git: `strata merge-file %O %A %B %P`)".into());
    };
    let path = args.pos.get(4).unwrap_or(a);
    // a side that doesn't exist (a file added on both sides has no base) is empty
    let read = |f: &str, side: &str| -> Result<Program, String> {
        let bytes = std::fs::read(f).map_err(|e| format!("{f}: {e}"))?;
        if bytes.is_empty() {
            return Ok(Program::default());
        }
        // each side alone: names from other modules stay in full in the text merged
        package::decode_file(std::path::Path::new(side), &bytes).map(|p| resolve::written_form(&p))
    };
    let sides = (read(o, "base"), read(a, "ours"), read(b, "theirs"));
    let (Ok(base), Ok(ours), Ok(theirs)) = sides else {
        let e = [sides.0.err(), sides.1.err(), sides.2.err()].into_iter().flatten().collect::<Vec<_>>().join("; ");
        eprintln!("strata: {path}: can't merge: {e}");
        return Ok(false);
    };
    let module = ours.decls.iter().chain(&theirs.decls).chain(&base.decls).map(|d| d.module.to_string()).next().unwrap_or_default();
    let text = |p: &Program| printer::module_file(p, &module);
    let m = merge::merge(&text(&base), &text(&ours), &text(&theirs))?;
    for n in &m.notes {
        eprintln!("strata: {path}: {n}");
    }
    if !m.conflicts.is_empty() {
        std::fs::write(a, &m.text).map_err(|e| format!("{a}: {e}"))?;
        for c in &m.conflicts {
            eprintln!("strata: {path}: conflict in {c}");
        }
        eprintln!("strata: {path} is left as text with conflict markers: fix it, then `strata resolve {path}` stores it");
        return Ok(false);
    }
    let merged = parser::parse_module_file(&m.text, &module).map_err(|e| format!("{path}: the merge doesn't parse ({e})"))?;
    std::fs::write(a, store::encode_at(std::path::Path::new(path), merged.decls.clone())?).map_err(|e| format!("{a}: {e}"))?;
    // under `strata sync`, the check waits until every file is merged and the
    // logged renames are followed: while git merges, the other files are still ours
    if std::env::var_os("STRATA_MERGE_BASE").is_some() {
        return Ok(true);
    }
    match merge::new_errors(path, &merged, &ours, &theirs) {
        Ok(errs) if !errs.is_empty() => {
            eprintln!("strata: {path}: merged definition by definition, but the result doesn't check:");
            for e in errs {
                eprintln!("    {e}");
            }
            Ok(false)
        }
        Ok(_) => Ok(true),
        Err(e) => {
            eprintln!("strata: {path}: not type-checked: {e}");
            Ok(true)
        }
    }
}

/// `strata resolve FILE`: a module file left as text by a merge conflict,
/// fixed by hand, stored again as a module file.
pub fn resolve(args: &Args) -> Result<bool, String> {
    let f = args.pos.get(1).ok_or("usage: strata resolve <FILE.strata>  (a file a merge left as text)")?;
    let path = std::path::Path::new(f);
    let bytes = std::fs::read(path).map_err(|e| format!("{f}: {e}"))?;
    if binary::is_binary(&bytes) {
        println!("{f} is already a module file");
        return Ok(true);
    }
    let text = String::from_utf8(bytes).map_err(|_| format!("{f}: neither a module file nor text"))?;
    if text.lines().any(|l| l.starts_with("<<<<<<<") || l.starts_with(">>>>>>>")) {
        return Err(format!("{f} still has conflict markers"));
    }
    let module = match store::module_at(path)? {
        Some((_, m)) => m,
        None => String::new(),
    };
    let p = parser::parse_module_file(&text, &module).map_err(|e| format!("{f}:{e}"))?;
    let n = p.decls.len();
    std::fs::write(path, store::encode_at(path, p.decls)?).map_err(|e| format!("{f}: {e}"))?;
    println!("{f}: {n} definitions stored");
    Ok(true)
}

/// `strata setup-git`.
pub fn setup_git() -> Result<bool, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let driver = format!("'{}' merge-file %O %A %B %P", exe.display());
    crate::git::run(None, &["config", "merge.strata.name", "Strata: merge each definition separately, then type-check"])?;
    crate::git::run(None, &["config", "merge.strata.driver", &driver])?;
    println!("git will merge .strata files with: {driver}");
    // `git diff` and `git log -p` show module files as their text
    let textconv = format!("'{}' render --file", exe.display());
    crate::git::run(None, &["config", "diff.strata.textconv", &textconv])?;
    println!("git will show .strata files as text with: {textconv}");
    let top = crate::git::run(None, &["rev-parse", "--show-toplevel"])?;
    let attrs = std::fs::read_to_string(std::path::Path::new(top.trim()).join(".gitattributes")).unwrap_or_default();
    let has = |w: &str| attrs.lines().any(|l| l.starts_with("*.strata") && l.split_whitespace().any(|x| x == w));
    if !has("merge=strata") || !has("diff=strata") {
        println!("add `*.strata binary merge=strata diff=strata` to .gitattributes (and commit it) so git uses them");
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
/// something, and every `.strata` file is in canonical form (the bytes strata
/// writes for it). `fix` rewrites the files that aren't.
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
        let p = match package::program(path) {
            Ok(p) => p,
            Err(e) => {
                ok = false;
                println!("✗ {e}");
                continue;
            }
        };
        // canonical: each file is exactly what strata would write for it
        let canonical: Vec<(std::path::PathBuf, Vec<u8>)> = if pkg.is_some() { store::encode_files(&p) } else { vec![(std::path::PathBuf::new(), store::encode_script(&p))] };
        for (rel, bytes) in package::files(path)? {
            let file = if rel.as_os_str().is_empty() { shown.clone() } else { format!("{shown}/{}", rel.display()) };
            let want = canonical.iter().find(|(r, _)| *r == rel).map(|(_, b)| b);
            if want == Some(&bytes) {
                continue;
            }
            if !fix {
                ok = false;
                println!("✗ {file}: not in canonical form (`strata ci --fix` rewrites it)");
            } else if let Some(want) = want {
                store::write_file(&top.join(&file), want)?;
                println!("  {file}: rewritten in canonical form");
            } else {
                ok = false;
                println!("✗ {file}: holds no module of its own (named after its path)");
            }
        }
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
