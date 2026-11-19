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
use crate::{parser, printer, resolve};
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

    // grouped by module, as `printer::program` does
    let mut mods: Vec<String> = Vec::new();
    for (m, _) in &pieces {
        if !mods.contains(m) {
            mods.push(m.clone());
        }
    }
    let mut out = Vec::new();
    for m in mods {
        let body: Vec<String> = pieces
            .iter()
            .filter(|(pm, _)| *pm == m)
            .map(|(_, p)| match p {
                Piece::Decl(d) => canon(d),
                Piece::Text(t) => t.clone(),
            })
            .collect();
        if m.is_empty() {
            out.extend(body);
        } else {
            out.push(printer::module_block(&m, &body.join("\n\n")));
        }
    }
    let text = if out.is_empty() { String::new() } else { out.join("\n\n") + "\n" };
    Ok(Merged { text, conflicts, notes })
}

/// `strata.toml`: the programs in this repository, each a file or a folder
/// of modules, relative to the file's directory.
pub struct Manifest {
    pub dir: std::path::PathBuf,
    pub programs: Vec<String>,
}

pub fn manifest(start: &Path) -> Result<Option<Manifest>, String> {
    let start = std::fs::canonicalize(start).unwrap_or(start.to_path_buf());
    let Some(dir) = start.ancestors().find(|d| d.join("strata.toml").is_file()) else { return Ok(None) };
    let src = std::fs::read_to_string(dir.join("strata.toml")).map_err(|e| format!("strata.toml: {e}"))?;
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Raw {
        programs: Vec<String>,
    }
    let raw: Raw = toml::from_str(&src).map_err(|e| format!("strata.toml: {e}"))?;
    Ok(Some(Manifest { dir: dir.to_path_buf(), programs: raw.programs.into_iter().map(|p| p.trim_end_matches('/').to_string()).collect() }))
}

impl Manifest {
    /// The program a file (relative to the manifest's directory) belongs to.
    pub fn program_of(&self, rel: &str) -> Option<&str> {
        self.programs.iter().map(|p| p.as_str()).filter(|p| rel == *p || rel.starts_with(&format!("{p}/"))).max_by_key(|p| p.len())
    }
}

/// The `.strata` files of a program, relative to the program's folder (or the
/// file itself, as `""`), with their text.
pub fn program_files(path: &Path) -> Result<Vec<(std::path::PathBuf, String)>, String> {
    if !path.is_dir() {
        let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        return Ok(vec![(std::path::PathBuf::new(), src)]);
    }
    let mut files = Vec::new();
    let mut todo = vec![path.to_path_buf()];
    while let Some(dir) = todo.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let p = entry.map_err(|e| e.to_string())?.path();
            if p.is_dir() {
                if !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
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

/// Parse a program's files (as `program_files` gives them).
pub fn parse_program_files(files: Vec<(std::path::PathBuf, String)>) -> Result<Program, String> {
    match files.as_slice() {
        [(rel, src)] if rel.as_os_str().is_empty() => parser::parse_module_file(src, ""),
        _ => crate::parse_files(files),
    }
}

/// The type errors of the merged file that neither side had on its own:
/// what the merge broke. The file is checked inside its program (from
/// `strata.toml`), with the program's other files as they are on disk.
pub fn new_errors(path: &str, merged: &str, ours: &str, theirs: &str) -> Result<Vec<String>, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let (files, slot) = match manifest(&cwd)? {
        Some(m) => {
            let full = std::fs::canonicalize(cwd.join(path)).unwrap_or(cwd.join(path));
            let rel = full.strip_prefix(&m.dir).map(|r| r.to_string_lossy().to_string()).unwrap_or(path.to_string());
            match m.program_of(&rel) {
                Some(prog) => {
                    let files = program_files(&m.dir.join(prog))?;
                    let inner = std::path::PathBuf::from(rel.strip_prefix(prog).unwrap_or("").trim_start_matches('/'));
                    let mut files: Vec<_> = files.into_iter().filter(|(r, _)| *r != inner).collect();
                    files.push((inner.clone(), String::new()));
                    (files, inner)
                }
                None => (vec![(std::path::PathBuf::new(), String::new())], std::path::PathBuf::new()),
            }
        }
        None => (vec![(std::path::PathBuf::new(), String::new())], std::path::PathBuf::new()),
    };
    let errors = |text: &str| -> Option<Vec<String>> {
        let fs = files.iter().map(|(r, s)| (r.clone(), if *r == slot { text.to_string() } else { s.clone() })).collect();
        let p = parse_program_files(fs).ok()?;
        Some(crate::full_check(&p).errors.iter().map(|e| format!("{}: {}", e.decl, e.msg)).collect())
    };
    let Some(mine) = errors(merged) else { return Err("the merged file doesn't parse".into()) };
    let known: Vec<String> = errors(ours).unwrap_or_default().into_iter().chain(errors(theirs).unwrap_or_default()).collect();
    Ok(mine.into_iter().filter(|e| !known.contains(e)).collect())
}
