//! A change in progress. An `edit` that breaks code elsewhere (a signature
//! changed, a definition removed) is not rejected: each broken use becomes a
//! typed hole (`?pending_1`) that remembers what it replaced and why, and the
//! package enters a pending change. Further edits, renames and moves join it;
//! `strata done` records the whole change as one log entry once its holes are
//! filled (or keeps them with `--keep-holes`), and `strata abort` puts every
//! file back as it was before the change started.
//!
//! The state is `.strata/pending.json` (local, never committed): the files as
//! they were, the holes, and the renames made along the way.

use crate::names::short;
use crate::ast::*;
use crate::store::Codebase;
use crate::types::Diag;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const FILE: &str = "pending.json";

/// Hole names of a pending change start with this; `strata ci` refuses them.
pub const PREFIX: &str = "pending_";

#[derive(Serialize, Deserialize, Clone)]
pub struct Hole {
    pub name: String,
    /// the definition it is in
    pub decl: String,
    /// the code it replaced
    pub was: String,
    /// why that code broke
    pub why: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Pending {
    /// what started it (the first edit's message)
    pub message: String,
    pub started: u64,
    /// the package's files before the change: (path in the package, text)
    pub before: Vec<(String, String)>,
    pub holes: Vec<Hole>,
    /// renames and moves made during the change, for its log entry
    pub renames: Vec<serde_json::Value>,
}

pub fn load(cb: &Codebase) -> Option<Pending> {
    let text = std::fs::read_to_string(cb.cache_dir().join(FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save(cb: &Codebase, p: &Pending) -> Result<(), String> {
    std::fs::write(cb.cache_dir().join(FILE), serde_json::to_string_pretty(p).unwrap()).map_err(|e| e.to_string())
}

pub fn clear(cb: &Codebase) {
    let _ = std::fs::remove_file(cb.cache_dir().join(FILE));
}

/// Start a pending change: remember the files as they are.
pub fn start(cb: &Codebase, message: &str) -> Result<Pending, String> {
    let before = crate::package::files(cb.root())?.into_iter().map(|(r, t)| (r.to_string_lossy().to_string(), t)).collect();
    let started = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    Ok(Pending { message: message.to_string(), started, before, holes: vec![], renames: vec![] })
}

/// The program before the change.
pub fn before_program(p: &Pending) -> Result<Program, String> {
    crate::package::parse(p.before.iter().map(|(r, t)| (std::path::PathBuf::from(r), t.clone())).collect())
}

/// Put the files back as they were before the change.
pub fn restore(cb: &Codebase, p: &Pending) -> Result<(), String> {
    let keep: HashSet<&str> = p.before.iter().map(|(r, _)| r.as_str()).collect();
    for (rel, _) in crate::package::files(cb.root())? {
        if !keep.contains(rel.to_string_lossy().as_ref()) {
            std::fs::remove_file(cb.root().join(&rel)).map_err(|e| format!("{}: {e}", rel.display()))?;
        }
    }
    for (rel, text) in &p.before {
        let path = cb.root().join(rel);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, text).map_err(|e| format!("{rel}: {e}"))?;
    }
    Ok(())
}

/// The names a change to these definitions can break, as written: each
/// changed or removed definition's short name, and its variants and methods.
fn changed_names(old: &Program, new: &Program, edited: &[String]) -> HashSet<String> {
    let mut out = HashSet::new();
    for q in edited {
        let interface = |p: &Program| p.decls.iter().find(|d| d.qname() == *q).map(crate::store::interface_hash);
        if interface(old) == interface(new) {
            continue;
        }
        for p in [old, new] {
            if let Some(d) = p.decls.iter().find(|d| d.qname() == *q) {
                out.insert(d.name());
                match &d.kind {
                    DeclKind::Type(t) => {
                        if let TypeBody::Sum(vs) = &t.body {
                            out.extend(vs.iter().map(|v| v.name.to_string()));
                        }
                    }
                    DeclKind::Trait(t) => out.extend(t.methods.iter().map(|m| m.decl.name.to_string())),
                    _ => {}
                }
            }
        }
    }
    out
}


/// Whether an expression uses one of `names` (a call, a reference, a
/// constructor, a method call, an `is` test).
fn uses(e: &Expr, names: &HashSet<String>) -> bool {
    let named = |n: &str| names.contains(short(n));
    match e {
        Expr::Var(n) | Expr::Ctor(n, _) | Expr::Method(_, n, _) | Expr::Is(_, n) => named(n),
        Expr::Call(f, _) => matches!(&**f, Expr::Var(n) if named(n)),
        _ => false,
    }
}

/// Replace, in a definition, each use of a changed name by a hole; the
/// outermost one wins. Patterns are left alone (they can't be holes). `make`
/// names the hole and records what it replaced.
fn holes_in(d: &mut DeclKind, names: &HashSet<String>, make: &mut dyn FnMut(&Expr) -> String) {
    struct Holes<'a> {
        names: &'a HashSet<String>,
        make: &'a mut dyn FnMut(&Expr) -> String,
    }
    impl VisitMut for Holes<'_> {
        fn expr(&mut self, e: &mut Expr) {
            if uses(e, self.names) {
                *e = Expr::Hole(Symbol::from((self.make)(e)));
            } else {
                walk_expr_mut(self, e)
            }
        }
        fn pattern(&mut self, _: &mut Pattern) {}
    }
    walk_decl_mut(&mut Holes { names, make }, d)
}

/// The variants a pattern names, anywhere in it.
fn pattern_ctors(p: &Pattern) -> Vec<String> {
    struct Ctors(Vec<String>);
    impl Visit for Ctors {
        fn pattern(&mut self, p: &Pattern) {
            if let Pattern::Ctor(c, _) = p {
                self.0.push(short(c).to_string());
            }
            walk_pattern(self, p)
        }
        fn expr(&mut self, _: &Expr) {}
    }
    let mut c = Ctors(Vec::new());
    c.pattern(p);
    c.0
}

/// A pattern without the alternatives that name a removed variant; `None`
/// when nothing of it is left.
fn without_removed(p: &Pattern, removed: &HashSet<String>) -> Option<Pattern> {
    if let Pattern::Or(alts) = p {
        let mut kept: Vec<Pattern> = alts.iter().filter_map(|a| without_removed(a, removed)).collect();
        return match kept.len() {
            0 => None,
            1 => kept.pop(),
            _ => Some(Pattern::Or(kept)),
        };
    }
    (!pattern_ctors(p).iter().any(|c| removed.contains(c))).then(|| p.clone())
}

/// A variant's changes for `match`: removed variants, and added ones with
/// the pattern that matches them (`Locked`, `Locked(_)`, `Locked { .. }`) and their type.
struct VariantChanges {
    removed: HashSet<String>,
    added: Vec<(String, String, Pattern)>,
}

fn variant_changes(old: &Program, new: &Program, edited: &[String]) -> VariantChanges {
    let variants = |p: &Program, q: &str| -> Vec<(String, Pattern)> {
        p.decls
            .iter()
            .find(|d| d.qname() == q)
            .and_then(|d| match &d.kind {
                DeclKind::Type(t) => match &t.body {
                    TypeBody::Sum(vs) => Some(
                        vs.iter()
                            .map(|v| {
                                let args = match &v.payload {
                                    Payload::None => CtorPat::None,
                                    Payload::Positional(ts) => CtorPat::Positional(vec![Pattern::Wild; ts.len()]),
                                    Payload::Record(_) => CtorPat::Record(vec![], true),
                                };
                                let pat = Pattern::Ctor(v.name.clone(), args);
                                (v.name.to_string(), pat)
                            })
                            .collect(),
                    ),
                    _ => None,
                },
                _ => None,
            })
            .unwrap_or_default()
    };
    let mut out = VariantChanges { removed: HashSet::new(), added: Vec::new() };
    for q in edited {
        let (before, after) = (variants(old, q), variants(new, q));
        if before.is_empty() {
            continue; // not a sum type before: new variants have no `match` to join
        }
        for (v, _) in &before {
            if !after.iter().any(|(x, _)| x == v) {
                out.removed.insert(v.clone());
            }
        }
        for (v, pat) in &after {
            if !before.iter().any(|(x, _)| x == v) {
                out.added.push((v.clone(), short(q).to_string(), pat.clone()));
            }
        }
    }
    out
}

/// `match`es after a variant change: arms of removed variants go (they can't
/// match any more), and a `match` on the type, without a catch-all, gets an
/// arm for each added variant whose body is a hole. A `match` with no arm
/// left becomes a hole. `make(was, why)` names a hole; `notes` says what went.
fn fix_matches(d: &mut DeclKind, ch: &VariantChanges, siblings: &HashMap<String, Vec<String>>, make: &mut dyn FnMut(String, String) -> String, notes: &mut Vec<String>) {
    struct Matches<F>(F);
    impl<F: FnMut(&mut Expr)> VisitMut for Matches<F> {
        fn expr(&mut self, e: &mut Expr) {
            (self.0)(e);
            walk_expr_mut(self, e)
        }
    }
    // every `match`, wherever it is (a refinement or a pattern's condition too)
    walk_decl_mut(&mut Matches(|e: &mut Expr| {
        let Expr::Match(_, _, arms) = e else { return };
        // removed variants
        let mut kept = Vec::new();
        for mut arm in std::mem::take(arms) {
            match without_removed(&arm.pat, &ch.removed) {
                Some(p) => {
                    if p != arm.pat {
                        let now = pattern_ctors(&p);
                        let gone: Vec<String> = pattern_ctors(&arm.pat).into_iter().filter(|c| !now.contains(c)).collect();
                        notes.push(format!("dropped `{}` (removed) from an arm", gone.join("`, `")));
                    }
                    arm.pat = p;
                    kept.push(arm);
                }
                None => {
                    let gone: Vec<String> = pattern_ctors(&arm.pat).into_iter().filter(|c| ch.removed.contains(c)).collect();
                    notes.push(format!("dropped the arm for `{}` (removed)", gone.join("`, `")));
                }
            }
        }
        // added variants, in a match on their type without a catch-all
        let tops: Vec<String> = kept
            .iter()
            .flat_map(|a| match &a.pat {
                Pattern::Or(alts) => alts.iter().collect(),
                p => vec![p],
            })
            .filter_map(|p| match p {
                Pattern::Ctor(c, _) => Some(short(c).to_string()),
                _ => None,
            })
            .collect();
        let catch_all = kept.iter().any(|a| matches!(a.pat, Pattern::Wild | Pattern::Bind(_)));
        if !catch_all {
            for (var, ty, pat) in &ch.added {
                let on_type = siblings.get(ty).is_some_and(|vs| vs.iter().any(|x| tops.contains(x)));
                if on_type && !tops.contains(var) {
                    let name = make(String::new(), format!("`{var}` is a new variant of `{ty}`"));
                    kept.push(Arm { comments: vec![], pat: pat.clone(), body: Expr::Hole(Symbol::from(name)) });
                }
            }
        }
        *arms = kept;
        if arms.is_empty() {
            let was = crate::printer::expr(e, 0);
            *e = Expr::Hole(Symbol::from(make(was, "every arm was for a removed variant".into())));
        }
    }), d)
}

/// After an edit of `edited` that leaves errors in other definitions: turn the
/// broken uses into holes. Gives the program with holes and the holes, or the
/// errors holes can't absorb (in the edited definitions themselves, or that
/// remain once the uses are holes).
pub fn holeify(old: &Program, new: &Program, edited: &[String], errors: &[Diag], first: usize) -> Result<(Program, Vec<Hole>, Vec<String>), Vec<Diag>> {
    // the checker names definitions as resolved; map them back
    let resolved = crate::resolve::resolve(new).program;
    let index: HashMap<String, usize> = resolved.decls.iter().enumerate().map(|(i, d)| (d.name(), i)).collect();
    let mut broken: Vec<usize> = Vec::new();
    let mut why: HashMap<usize, Vec<String>> = HashMap::new();
    for e in errors {
        match index.get(&e.decl) {
            Some(&i) if !edited.contains(&new.decls[i].qname()) => {
                if !broken.contains(&i) {
                    broken.push(i);
                }
                why.entry(i).or_default().push(e.msg.clone());
            }
            _ => return Err(errors.to_vec()), // the edit itself doesn't check
        }
    }
    let names = changed_names(old, new, edited);
    let ch = variant_changes(old, new, edited);
    // each changed type's variants (after), by the type's short name
    let siblings: HashMap<String, Vec<String>> = edited
        .iter()
        .filter_map(|q| new.decls.iter().find(|d| d.qname() == *q))
        .filter_map(|d| match &d.kind {
            DeclKind::Type(t) => match &t.body {
                TypeBody::Sum(vs) => Some((t.name.to_string(), vs.iter().map(|v| v.name.to_string()).collect())),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let mut out = new.clone();
    let holes = std::cell::RefCell::new(Vec::new());
    let mut notes = Vec::new();
    let k = std::cell::Cell::new(first);
    for i in broken {
        let d: &mut Decl = &mut out.decls[i];
        let decl = d.qname();
        let reasons = why.get(&i).cloned().unwrap_or_default();
        let new_hole = |was: String, why: Vec<String>| -> String {
            let name = format!("{PREFIX}{}", k.get());
            k.set(k.get() + 1);
            holes.borrow_mut().push(Hole { name: name.clone(), decl: decl.clone(), was, why });
            name
        };
        // matches first: arms of removed variants go, added variants get an arm
        let mut decl_notes = Vec::new();
        fix_matches(&mut d.kind, &ch, &siblings, &mut |was, why| new_hole(was, vec![why]), &mut decl_notes);
        notes.extend(decl_notes.into_iter().map(|n| format!("in {decl}: {n}")));
        // then the uses: calls, references, constructors, method calls, `is` tests
        holes_in(&mut d.kind, &names, &mut |e| new_hole(crate::printer::expr(e, 0), reasons.clone()));
    }
    let rep = crate::full_check(&out);
    if !rep.ok() {
        return Err(rep.errors);
    }
    Ok((out, holes.into_inner(), notes))
}

/// Rename the holes `?pending_k` to `?todo_k` (kept when a change is done with them).
pub fn keep_holes(p: &Program) -> Program {
    struct Keep;
    impl VisitMut for Keep {
        fn expr(&mut self, e: &mut Expr) {
            if let Expr::Hole(h) = e {
                if let Some(rest) = h.strip_prefix(PREFIX) {
                    *h = format!("todo_{rest}").into();
                }
            }
            walk_expr_mut(self, e)
        }
    }
    let mut out = p.clone();
    for d in &mut out.decls {
        walk_decl_mut(&mut Keep, &mut d.kind);
    }
    out
}
