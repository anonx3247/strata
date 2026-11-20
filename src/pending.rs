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

use crate::ast::{Decl, Expr, Program};
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
                    crate::ast::DeclKind::Type(t) => {
                        if let crate::ast::TypeBody::Sum(vs) = &t.body {
                            out.extend(vs.iter().map(|v| v.name.clone()));
                        }
                    }
                    crate::ast::DeclKind::Trait(t) => out.extend(t.methods.iter().map(|m| m.decl.name.clone())),
                    _ => {}
                }
            }
        }
    }
    out
}

fn last(n: &str) -> &str {
    n.rsplit('.').next().unwrap_or(n)
}

/// Replace, in a serialized expression tree, each use of a changed name (a
/// call, a reference, a constructor, a method call, an `is` test) by a hole;
/// the outermost one wins. `make` names the hole and records what it replaced.
fn holes_in(v: &mut serde_json::Value, names: &HashSet<String>, make: &mut dyn FnMut(&serde_json::Value) -> String) {
    use serde_json::Value as J;
    let hit = |v: &J| -> bool {
        let Some(o) = v.as_object().filter(|o| o.len() == 1) else { return false };
        let (k, x) = o.iter().next().unwrap();
        let named = |n: &J| n.as_str().is_some_and(|s| names.contains(last(s)));
        match k.as_str() {
            "Var" => named(x),
            "Call" => x.get(0).and_then(|f| f.get("Var")).is_some_and(named),
            "Ctor" => x.get(0).is_some_and(named),
            "Method" => x.get(1).is_some_and(named),
            "Is" => x.get(1).is_some_and(named),
            _ => false,
        }
    };
    if hit(v) {
        let name = make(v);
        *v = serde_json::json!({ "Hole": name });
        return;
    }
    match v {
        J::Object(o) => o.values_mut().for_each(|x| holes_in(x, names, make)),
        J::Array(a) => a.iter_mut().for_each(|x| holes_in(x, names, make)),
        _ => {}
    }
}

/// After an edit of `edited` that leaves errors in other definitions: turn the
/// broken uses into holes. Gives the program with holes and the holes, or the
/// errors holes can't absorb (in the edited definitions themselves, or that
/// remain once the uses are holes).
pub fn holeify(old: &Program, new: &Program, edited: &[String], errors: &[Diag], first: usize) -> Result<(Program, Vec<Hole>), Vec<Diag>> {
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
    let mut out = new.clone();
    let mut holes = Vec::new();
    let mut k = first;
    for i in broken {
        let d: &mut Decl = &mut out.decls[i];
        let decl = d.qname();
        let mut j = serde_json::to_value(&d.kind).map_err(|_| errors.to_vec())?;
        let reasons = why.get(&i).cloned().unwrap_or_default();
        let mut make = |node: &serde_json::Value| -> String {
            let name = format!("{PREFIX}{k}");
            k += 1;
            let was = serde_json::from_value::<Expr>(node.clone()).map(|e| crate::printer::expr(&e, 0)).unwrap_or_default();
            holes.push(Hole { name: name.clone(), decl: decl.clone(), was, why: reasons.clone() });
            name
        };
        holes_in(&mut j, &names, &mut make);
        d.kind = serde_json::from_value(j).map_err(|_| errors.to_vec())?;
    }
    let rep = crate::full_check(&out);
    if !rep.ok() {
        return Err(rep.errors);
    }
    Ok((out, holes))
}

/// Rename the holes `?pending_k` to `?todo_k` (kept when a change is done with them).
pub fn keep_holes(p: &Program) -> Program {
    fn walk(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(o) => {
                if let Some(serde_json::Value::String(h)) = o.get_mut("Hole") {
                    if let Some(rest) = h.strip_prefix(PREFIX) {
                        *h = format!("todo_{rest}");
                    }
                }
                o.values_mut().for_each(walk);
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(walk),
            _ => {}
        }
    }
    let mut out = p.clone();
    for d in &mut out.decls {
        let mut j = serde_json::to_value(&d.kind).unwrap();
        walk(&mut j);
        d.kind = serde_json::from_value(j).unwrap_or_else(|_| d.kind.clone());
    }
    out
}
