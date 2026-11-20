//! Three-way merge of a `.strata` file, one definition at a time: the git
//! merge driver (`strata merge-file %O %A %B %P`).
//!
//! Each side is parsed and its definitions are paired with the base by
//! qualified name. A definition only one side changed is taken from that side,
//! whatever the other side did elsewhere in the file. Two edits of the same
//! definition are merged line by line on its canonical text, and the result is
//! kept only if it parses back into that one definition; otherwise the file
//! gets conflict markers around that definition alone. Renames (a definition
//! gone and a new one that is the base's after `strata rename`) are carried
//! into the other side, so its new references follow the rename.

use crate::ast::{Decl, DeclKind, Program};
use crate::{package, parser, printer, resolve};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

pub struct Merged {
    /// the merged file: canonical text, with conflict markers if `conflicts` isn't empty
    pub text: String,
    /// definitions (or renames) that need a person
    pub conflicts: Vec<String>,
    /// what happened, one line each
    pub notes: Vec<String>,
}

enum Piece {
    Decl(Decl),
    /// conflict markers around one definition
    Text(String),
}

fn canon(d: &Decl) -> String {
    printer::decl(d)
}

fn short(q: &str) -> &str {
    q.rsplit('.').next().unwrap_or(q)
}

fn renamable(d: &Decl) -> bool {
    matches!(d.kind, DeclKind::Fn(_) | DeclKind::Type(_) | DeclKind::Trait(_))
}

/// Renames from `o` to `s`: a definition `s` no longer has, and one it added
/// that looks like the base's after the rename (body edits allowed).
fn renames(o: &Program, s: &Program) -> Vec<(String, String)> {
    let names = |p: &Program| p.decls.iter().map(|d| d.qname()).collect::<std::collections::HashSet<_>>();
    let (on, sn) = (names(o), names(s));
    let gone: Vec<&Decl> = o.decls.iter().filter(|d| renamable(d) && !sn.contains(&d.qname())).collect();
    let mut added: Vec<&Decl> = s.decls.iter().filter(|d| renamable(d) && !on.contains(&d.qname())).collect();
    let mut out = Vec::new();
    for x in gone {
        let mut best: Option<(f64, usize)> = None;
        for (i, y) in added.iter().enumerate() {
            if y.kind_str() != x.kind_str() || y.module != x.module {
                continue;
            }
            let Ok(r) = resolve::rename(o, &x.qname(), &y.name()) else { continue };
            let Some(ry) = r.decls.iter().find(|d| d.qname() == y.qname()) else { continue };
            let sim = if canon(ry) == canon(y) { 2.0 } else { crate::diff::similarity(&canon(ry), &canon(y)) };
            // an edited definition counts only with the rename's footprint: what
            // referred to the old name now refers to the new one
            let word = |t: &str, w: &str| t.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|t| t == w);
            let followed = o.decls.iter().zip(&r.decls).filter(|(od, rd)| od.qname() != x.qname() && canon(od) != canon(rd)).any(|(od, _)| {
                s.decls.iter().find(|sd| sd.qname() == od.qname()).is_some_and(|sd| word(&canon(sd), &y.name()) && !word(&canon(sd), &x.name()))
            });
            if (sim == 2.0 || sim >= 0.6 && followed) && best.is_none_or(|(b, _)| sim > b) {
                best = Some((sim, i));
            }
        }
        if let Some((_, i)) = best {
            out.push((x.qname(), added.remove(i).qname()));
        }
    }
    out
}

/// `git merge-file` on three texts: the merged text and whether it is clean.
fn line_merge(base: &str, ours: &str, theirs: &str) -> Result<(String, bool), String> {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("strata-merge-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let files = [("ours", ours), ("base", base), ("theirs", theirs)];
    for (n, t) in files {
        std::fs::write(dir.join(n), t).map_err(|e| e.to_string())?;
    }
    let o = Command::new("git")
        .args(["merge-file", "-p", "--diff3", "-L", "ours", "-L", "base", "-L", "theirs"])
        .args(files.map(|(n, _)| dir.join(n)))
        .output()
        .map_err(|e| format!("git merge-file: {e}"));
    let _ = std::fs::remove_dir_all(&dir);
    let o = o?;
    match o.status.code() {
        Some(c) if (0..128).contains(&c) => Ok((String::from_utf8_lossy(&o.stdout).to_string(), c == 0)),
        _ => Err(format!("git merge-file: {}", String::from_utf8_lossy(&o.stderr).trim())),
    }
}

/// Merge the whole file as text, for sides that don't parse.
pub fn text_merge(base: &str, ours: &str, theirs: &str) -> Result<(String, bool), String> {
    line_merge(base, ours, theirs)
}

pub fn merge(base: &str, ours: &str, theirs: &str) -> Result<Merged, String> {
    let parse = |src: &str, side: &str| parser::parse_module_file(src, "").map_err(|e| format!("{side} doesn't parse ({e})"));
    let (mut o, mut a, mut b) = (parse(base, "base")?, parse(ours, "ours")?, parse(theirs, "theirs")?);
    let mut notes = Vec::new();
    let mut conflicts = Vec::new();

    // carry each side's renames into the base and the other side
    let (ra, rb) = (renames(&o, &a), renames(&o, &b));
    for (from, to) in &ra {
        match rb.iter().find(|(f, _)| f == from) {
            Some((_, t)) if t == to => notes.push(format!("both sides renamed {from} → {to}")),
            Some((_, t)) => {
                conflicts.push(format!("{from} (renamed to {to} in ours, {t} in theirs)"));
                continue;
            }
            None => {}
        }
        o = resolve::rename(&o, from, short(to)).unwrap_or(o);
        if b.decls.iter().any(|d| &d.qname() == from) {
            match resolve::rename(&b, from, short(to)) {
                Ok(r) => {
                    b = r;
                    notes.push(format!("ours renamed {from} → {to}; applied to theirs"));
                }
                Err(e) => notes.push(format!("ours renamed {from} → {to}, but theirs can't follow it: {e}")),
            }
        }
    }
    for (from, to) in &rb {
        if ra.iter().any(|(f, _)| f == from) {
            continue;
        }
        o = resolve::rename(&o, from, short(to)).unwrap_or(o);
        if a.decls.iter().any(|d| &d.qname() == from) {
            match resolve::rename(&a, from, short(to)) {
                Ok(r) => {
                    a = r;
                    notes.push(format!("theirs renamed {from} → {to}; applied to ours"));
                }
                Err(e) => notes.push(format!("theirs renamed {from} → {to}, but ours can't follow it: {e}")),
            }
        }
    }

    // ours' order; a definition only theirs has goes after the one before it in theirs
    let mut order: Vec<String> = a.decls.iter().map(|d| d.qname()).collect();
    for (i, d) in b.decls.iter().enumerate() {
        let q = d.qname();
        if order.contains(&q) {
            continue;
        }
        let at = b.decls[..i].iter().rev().find_map(|p| order.iter().position(|n| *n == p.qname())).map_or(0, |k| k + 1);
        order.insert(at, q);
    }
    let get = |p: &Program| p.decls.iter().map(|d| (d.qname(), d.clone())).collect::<HashMap<_, _>>();
    let (om, am, bm) = (get(&o), get(&a), get(&b));
    let same = |x: &Decl, y: &Decl| canon(x) == canon(y);

    let mut pieces: Vec<(String, Piece)> = Vec::new();
    let (mut from_theirs, mut by_line) = (0, 0);
    for q in &order {
        let (od, ad, bd) = (om.get(q), am.get(q), bm.get(q));
        let module = ad.or(bd).or(od).map(|d| d.module.clone()).unwrap_or_default();
        let both = |o: Option<&Decl>, a: &Decl, b: &Decl, pieces: &mut Vec<(String, Piece)>, conflicts: &mut Vec<String>, by_line: &mut usize| -> Result<(), String> {
            let ot = o.map(canon).unwrap_or_default();
            let nl = |t: String| if t.is_empty() { t } else { t + "\n" };
            let (text, clean) = line_merge(&nl(ot), &nl(canon(a)), &nl(canon(b)))?;
            if clean {
                if let Ok(p) = parser::parse_module_file(&text, "") {
                    if let [d] = p.decls.as_slice() {
                        if d.name() == a.name() {
                            let mut d = d.clone();
                            d.module = a.module.clone();
                            d.public = d.public || a.public && b.public;
                            *by_line += 1;
                            pieces.push((a.module.clone(), Piece::Decl(d)));
                            return Ok(());
                        }
                    }
                }
            }
            conflicts.push(a.qname());
            pieces.push((a.module.clone(), Piece::Text(text.trim_end().to_string())));
            Ok(())
        };
        let gone = |kept: &Decl, kept_side: &str, o: &Decl| {
            let part = |t: String| if t.is_empty() { t } else { t + "\n" };
            let (ours, theirs) = if kept_side == "ours" { (canon(kept), String::new()) } else { (String::new(), canon(kept)) };
            let (lo, lt) = if kept_side == "ours" { ("", " (deleted)") } else { (" (deleted)", "") };
            format!("<<<<<<< ours{lo}\n{}||||||| base\n{}=======\n{}>>>>>>> theirs{lt}", part(ours), part(canon(o)), part(theirs))
        };
        match (od, ad, bd) {
            (_, Some(a), Some(b)) if same(a, b) => pieces.push((module, Piece::Decl(a.clone()))),
            (Some(o), Some(a), Some(b)) if same(o, a) => {
                from_theirs += 1;
                pieces.push((module, Piece::Decl(b.clone())))
            }
            (Some(o), Some(a), Some(b)) if same(o, b) => pieces.push((module, Piece::Decl(a.clone()))),
            (o, Some(a), Some(b)) => both(o, a, b, &mut pieces, &mut conflicts, &mut by_line)?,
            (Some(o), Some(a), None) if !same(o, a) => {
                conflicts.push(format!("{q} (changed in ours, deleted in theirs)"));
                pieces.push((module, Piece::Text(gone(a, "ours", o))));
            }
            (Some(o), None, Some(b)) if !same(o, b) => {
                conflicts.push(format!("{q} (deleted in ours, changed in theirs)"));
                pieces.push((module, Piece::Text(gone(b, "theirs", o))));
            }
            (Some(_), _, _) => {} // deleted on one side and untouched on the other, or on both
            (None, Some(a), None) => pieces.push((module, Piece::Decl(a.clone()))),
            (None, None, Some(b)) => {
                from_theirs += 1;
                pieces.push((module, Piece::Decl(b.clone())))
            }
            (None, None, None) => {}
        }
    }
    if from_theirs > 0 {
        notes.push(format!("{from_theirs} definition{} from theirs", if from_theirs == 1 { "" } else { "s" }));
    }
    if by_line > 0 {
        notes.push(format!("{by_line} definition{} changed on both sides, merged line by line", if by_line == 1 { "" } else { "s" }));
    }

    // a file is one module: its definitions in order, as `printer::program` writes them
    let out: Vec<String> = pieces
        .iter()
        .map(|(_, p)| match p {
            Piece::Decl(d) => canon(d),
            Piece::Text(t) => t.clone(),
        })
        .collect();
    let text = if out.is_empty() { String::new() } else { out.join("\n\n") + "\n" };
    Ok(Merged { text, conflicts, notes })
}

/// The type errors of the merged file that neither side had on its own:
/// what the merge broke. The file is checked inside its package, with the
/// package's other files as they are on disk; a script is checked alone.
pub fn new_errors(path: &str, merged: &str, ours: &str, theirs: &str) -> Result<Vec<String>, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let full = cwd.join(path);
    let parent = full.parent().unwrap_or(&cwd);
    let (files, slot) = match package::find(parent)? {
        Some(pkg) => {
            let full = std::fs::canonicalize(parent).unwrap_or(parent.to_path_buf()).join(full.file_name().unwrap_or_default());
            let inner = full.strip_prefix(&pkg.dir).map(|r| r.to_path_buf()).unwrap_or_default();
            let mut files: Vec<_> = package::files(&pkg.dir)?.into_iter().filter(|(r, _)| *r != inner).collect();
            files.push((inner.clone(), String::new()));
            (files, inner)
        }
        None => (vec![(std::path::PathBuf::new(), String::new())], std::path::PathBuf::new()),
    };
    let errors = |text: &str| -> Option<Vec<String>> {
        let fs = files.iter().map(|(r, s)| (r.clone(), if *r == slot { text.to_string() } else { s.clone() })).collect();
        let p = package::parse(fs).ok()?;
        Some(crate::full_check(&p).errors.iter().map(|e| format!("{}: {}", e.decl, e.msg)).collect())
    };
    let Some(mine) = errors(merged) else { return Err("the merged file doesn't parse".into()) };
    let known: Vec<String> = errors(ours).unwrap_or_default().into_iter().chain(errors(theirs).unwrap_or_default()).collect();
    Ok(mine.into_iter().filter(|e| !known.contains(e)).collect())
}

/// The references a merge left to a name that one side renamed: with the
/// renamed definition back under its old name, `resolve::rename` finds every
/// reference to it (including the other side's new ones) and renames them.
/// `None` when there is nothing to follow.
pub fn follow_rename(p: &Program, from: &str, to: &str) -> Option<Program> {
    // a move (`strata move`): the same short name, in another module
    if short(from) == short(to) && from != to {
        return resolve::follow_move(p, from, to);
    }
    if resolve::member(p, from).is_some() {
        return None; // the old name is defined again (the other side kept or re-added it)
    }
    let (kind, _) = resolve::member(p, to)?;
    let mut back = p.clone();
    resolve::set_member_name(&mut back, to, short(from));
    // `use` lists that name the new name (the renaming side's, kept by the
    // merge) name the old one again, so the other side's bare references resolve
    let module = to.rsplit_once('.').map(|(m, _)| m).unwrap_or("");
    for d in &mut back.decls {
        if let DeclKind::Use(u) = &mut d.kind {
            if u.path == module {
                for item in u.items.iter_mut().flatten() {
                    if item == short(to) {
                        *item = short(from).to_string();
                    }
                }
            }
        }
    }
    // a field isn't resolved by name: rename it again (it refuses an ambiguous one)
    let r = if kind == "field" { resolve::rename(&back, from, short(to)).ok()? } else { resolve::follow(p, &back, from, short(to)).ok()? };
    let changed = r.decls.iter().zip(&p.decls).any(|(a, b)| canon(a) != canon(b));
    changed.then_some(r)
}

/// The renames each side of a merge recorded in a package's log since
/// `base`: (from, to, "ours" | "theirs"). `log` is the log's path from the
/// repository's top.
pub fn logged_renames(top: &Path, log: &str, base: &str, theirs: &str) -> Vec<(String, String, &'static str)> {
    let show = |rev: &str| -> Vec<serde_json::Value> {
        Command::new("git")
            .arg("-C")
            .arg(top)
            .args(["show", &format!("{rev}:{log}")])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| crate::store::parse_log(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default()
    };
    let known: std::collections::HashSet<String> = show(base).iter().filter_map(|e| e["id"].as_str().map(|s| s.to_string())).collect();
    let mut out = Vec::new();
    for (rev, side) in [("HEAD", "ours"), (theirs, "theirs")] {
        for e in show(rev) {
            if e["id"].as_str().is_some_and(|id| known.contains(id)) {
                continue;
            }
            for (f, t) in crate::store::renames_of(&e) {
                out.push((f, t, side));
            }
        }
    }
    out
}

/// The package log's path from the repository's top, for a package folder.
fn log_rel(top: &Path, dir: &Path) -> String {
    let top = std::fs::canonicalize(top).unwrap_or(top.to_path_buf());
    let dir = std::fs::canonicalize(dir).unwrap_or(dir.to_path_buf());
    dir.strip_prefix(&top).unwrap_or(&dir).join(crate::store::LOG).to_string_lossy().replace('\\', "/")
}

/// After a merge: renames recorded in either side's log since `base` are
/// applied to every package in the working tree, so the other side's new
/// references, in any file, follow them. Each package that changed gets a
/// log entry and is staged. Returns what was done, one line each.
pub fn follow_renames(top: &Path, base: &str, theirs: &str) -> Result<Vec<String>, String> {
    let mut notes = Vec::new();
    let (packages, _) = package::discover(top)?;
    for dir in packages {
        let renames = logged_renames(top, &log_rel(top, &dir), base, theirs);
        if renames.is_empty() {
            continue;
        }
        let mut cb = crate::store::Codebase::open(&dir)?;
        for (from, to, side) in renames {
            let Some(next) = follow_rename(&cb.program, &from, &to) else { continue };
            let before: HashMap<String, String> = cb.program.decls.iter().map(|d| (d.qname(), canon(d))).collect();
            let updated: Vec<String> = next.decls.iter().filter(|d| before.get(&d.qname()).is_some_and(|t| *t != canon(d))).map(|d| d.qname()).collect();
            let other = if side == "ours" { "theirs" } else { "ours" };
            let msg = format!("merge: {other} code follows the rename {from} → {to}");
            cb.commit_with(next, &msg, serde_json::json!({"follows": {"from": from, "to": to, "references": updated}}))?;
            notes.push(format!("{msg} ({})", updated.join(", ")));
        }
        let o = Command::new("git").arg("-C").arg(top).args(["add", "--"]).arg(&dir).output().map_err(|e| format!("git add: {e}"))?;
        if !o.status.success() {
            return Err(format!("git add {}: {}", dir.display(), String::from_utf8_lossy(&o.stderr).trim()));
        }
    }
    Ok(notes)
}
