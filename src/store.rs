//! The codebase database. Definitions are stored as JSON syntax trees,
//! addressed by the hash of their canonical form. Names point at hashes.
//! Every change is an atomic, checked patch recorded in the log.

use crate::ast::*;
use crate::printer;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

pub const DIR: &str = ".strata";

#[derive(Serialize, Deserialize, Default)]
struct Index {
    /// (name, hash) in declaration order
    defs: Vec<(String, String)>,
}

pub struct Codebase {
    root: PathBuf,
    pub program: Program,
}

#[derive(Serialize, Debug)]
pub struct Change {
    pub name: String,
    pub kind: String,
    pub old: Option<String>,
    pub new: Option<String>,
    pub interface_changed: bool,
    /// Definitions that must be re-checked because they depend on this one.
    pub affected: Vec<String>,
}

pub fn sha(s: &str) -> String {
    let d = Sha256::digest(s.as_bytes());
    d.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Hash of a definition's canonical text (its identity in the store).
pub fn self_hash(d: &Decl) -> String {
    sha(&format!("module {}\n{}", d.module, printer::decl(d)))
}

/// Hash of what other definitions may depend on: the signature, contracts and type shapes.
pub fn interface_hash(d: &Decl) -> String {
    let text = match &d.kind {
        DeclKind::Fn(f) => printer::fn_signature(f),
        DeclKind::Test(t) => format!("test {:?}", t.name),
        _ => printer::decl(d),
    };
    sha(&format!("module {} pub {}\n{text}", d.module, d.public))
}

/// Names of other top-level definitions this one refers to.
pub fn refs(d: &Decl, ctor_types: &HashMap<String, String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    fn ty(t: &TypeExpr, out: &mut BTreeSet<String>) {
        match t {
            TypeExpr::Named(n, args) => {
                out.insert(n.clone());
                args.iter().for_each(|a| ty(a, out));
            }
            TypeExpr::Fn(ps, r) => {
                ps.iter().for_each(|a| ty(a, out));
                ty(r, out);
            }
            TypeExpr::Unit => {}
            TypeExpr::Refined(b, _) => ty(b, out),
        }
    }
    fn pat(p: &Pattern, out: &mut BTreeSet<String>, ct: &HashMap<String, String>) {
        match p {
            Pattern::Ctor(c, cp) => {
                out.insert(ct.get(c).cloned().unwrap_or(c.clone()));
                match cp {
                    CtorPat::Positional(ps) => ps.iter().for_each(|p| pat(p, out, ct)),
                    CtorPat::Record(fs, _) => fs.iter().for_each(|(_, p)| pat(p, out, ct)),
                    CtorPat::None => {}
                }
            }
            Pattern::List(ps, _) | Pattern::Tuple(ps) => ps.iter().for_each(|p| pat(p, out, ct)),
            _ => {}
        }
    }
    fn ex(e: &Expr, out: &mut BTreeSet<String>, ct: &HashMap<String, String>) {
        let go = |x: &Expr, out: &mut BTreeSet<String>| ex(x, out, ct);
        match e {
            Expr::Var(v) => {
                out.insert(v.clone());
            }
            Expr::List(xs) | Expr::Tuple(xs) => xs.iter().for_each(|x| go(x, out)),
            Expr::Ctor(c, args) => {
                out.insert(ct.get(c).cloned().unwrap_or(c.clone()));
                match args {
                    CtorArgs::Positional(xs) => xs.iter().for_each(|x| go(x, out)),
                    CtorArgs::Record(fs) => fs.iter().for_each(|(_, x)| go(x, out)),
                    CtorArgs::Update(b, fs) => {
                        go(b, out);
                        fs.iter().for_each(|(_, x)| go(x, out));
                    }
                    CtorArgs::None => {}
                }
            }
            Expr::Call(f, xs) => {
                go(f, out);
                xs.iter().for_each(|x| go(x, out));
            }
            Expr::Perform(eff, _, xs) => {
                out.insert(eff.clone());
                xs.iter().for_each(|x| go(x, out));
            }
            Expr::Method(r, m, xs) => {
                out.insert(m.clone());
                go(r, out);
                xs.iter().for_each(|x| go(x, out));
            }
            Expr::Field(r, _) | Expr::Un(_, r) | Expr::Lambda(_, r) => go(r, out),
            Expr::Is(r, c) => {
                out.insert(ct.get(c).cloned().unwrap_or(c.clone()));
                go(r, out);
            }
            Expr::Bin(_, l, r) => {
                go(l, out);
                go(r, out);
            }
            Expr::If(c, t, f) => {
                go(c, out);
                go(t, out);
                go(f, out);
            }
            Expr::Match(s, arms) => {
                go(s, out);
                for a in arms {
                    pat(&a.pat, out, ct);
                    go(&a.body, out);
                }
            }
            Expr::Block(stmts, last) => {
                for s in stmts {
                    match s {
                        Stmt::Let(p, t, x) => {
                            pat(p, out, ct);
                            if let Some(t) = t {
                                ty(t, out);
                            }
                            go(x, out);
                        }
                        Stmt::Expr(x) | Stmt::Assert(x) => go(x, out),
                    }
                }
                go(last, out);
            }
            Expr::Handle(b, hs) => {
                go(b, out);
                for h in hs {
                    out.insert(h.effect.clone());
                    if let HandlerKind::Clauses(cs) = &h.kind {
                        cs.iter().for_each(|c| go(&c.body, out));
                    }
                    if let Some((_, init)) = &h.state {
                        go(init, out);
                    }
                    if let Some((_, b)) = &h.on_return {
                        go(b, out);
                    }
                }
            }
            _ => {}
        }
    }
    match &d.kind {
        DeclKind::Type(t) => {
            match &t.body {
                TypeBody::Alias(a) => ty(a, &mut out),
                TypeBody::Record(fs) => fs.iter().for_each(|f| ty(&f.ty, &mut out)),
                TypeBody::Sum(vs) => vs.iter().for_each(|v| match &v.payload {
                    Payload::Positional(ts) => ts.iter().for_each(|t| ty(t, &mut out)),
                    Payload::Record(fs) => fs.iter().for_each(|f| ty(&f.ty, &mut out)),
                    Payload::None => {}
                }),
            }
            if let Some(r) = &t.refinement {
                ex(r, &mut out, ctor_types);
            }
            out.remove(&t.name);
        }
        DeclKind::Effect(e) => e.ops.iter().for_each(|o| {
            o.params.iter().for_each(|p| ty(&p.ty, &mut out));
            ty(&o.ret, &mut out);
        }),
        DeclKind::Fn(f) => {
            f.params.iter().for_each(|p| ty(&p.ty, &mut out));
            ty(&f.ret, &mut out);
            f.effects.iter().for_each(|e| ty(e, &mut out));
            f.requires.iter().chain(&f.ensures).for_each(|e| ex(e, &mut out, ctor_types));
            ex(&f.body, &mut out, ctor_types);
            out.remove(&f.name);
        }
        DeclKind::Trait(t) => {
            t.supers.iter().for_each(|s| ty(s, &mut out));
            for m in &t.methods {
                m.decl.params.iter().for_each(|p| ty(&p.ty, &mut out));
                ty(&m.decl.ret, &mut out);
                m.decl.requires.iter().chain(&m.decl.ensures).for_each(|e| ex(e, &mut out, ctor_types));
                if m.default {
                    ex(&m.decl.body, &mut out, ctor_types);
                }
            }
            for l in &t.laws {
                l.params.iter().for_each(|p| ty(&p.ty, &mut out));
                ex(&l.body, &mut out, ctor_types);
            }
            out.remove(&t.name);
            out.remove("Self");
        }
        DeclKind::Impl(i) => {
            out.insert(i.trait_name.clone());
            i.trait_args.iter().for_each(|a| ty(a, &mut out));
            ty(&i.for_type, &mut out);
            for m in &i.methods {
                m.params.iter().for_each(|p| ty(&p.ty, &mut out));
                ty(&m.ret, &mut out);
                m.effects.iter().for_each(|e| ty(e, &mut out));
                ex(&m.body, &mut out, ctor_types);
            }
            out.remove("Self");
        }
        DeclKind::Test(t) => ex(&t.body, &mut out, ctor_types),
        DeclKind::Use(_) => {}
    }
    out
}

/// Dependency graph restricted to names defined in the program.
pub fn graph(p: &Program) -> BTreeMap<String, BTreeSet<String>> {
    // compute on resolved (qualified) names, report by each definition's qname
    let original = p;
    let resolved = crate::resolve::resolve(p);
    let p = &resolved.program;
    let to_q: HashMap<String, String> = p.decls.iter().zip(&original.decls).map(|(r, d)| (r.name(), d.qname())).collect();
    let names: BTreeSet<String> = p.decls.iter().map(|d| d.name()).collect();
    let mut ct = HashMap::new();
    for d in &p.decls {
        if let DeclKind::Type(t) = &d.kind {
            if let TypeBody::Sum(vs) = &t.body {
                for v in vs {
                    ct.insert(v.name.clone(), t.name.clone());
                }
            }
        }
    }
    p.decls
        .iter()
        .map(|d| (to_q[&d.name()].clone(), refs(d, &ct).into_iter().filter(|r| names.contains(r) && r != &d.name()).map(|r| to_q[&r].clone()).collect()))
        .collect()
}

/// Hash of a definition together with everything it (transitively) depends on.
/// Cached verification results are keyed by this: if nothing reachable changed,
/// the result still holds.
pub fn closure_hash(p: &Program, name: &str) -> String {
    let g = graph(p);
    let hashes: HashMap<String, String> = p.decls.iter().map(|d| (d.qname(), self_hash(d))).collect();
    let mut seen = BTreeSet::new();
    let mut todo = vec![name.to_string()];
    while let Some(n) = todo.pop() {
        if seen.insert(n.clone()) {
            todo.extend(g.get(&n).into_iter().flatten().cloned());
        }
    }
    sha(&seen.iter().map(|n| format!("{n}={}", hashes.get(n).cloned().unwrap_or_default())).collect::<Vec<_>>().join(";"))
}

/// Everything that (transitively) depends on `name`.
pub fn dependents(p: &Program, name: &str) -> Vec<String> {
    let g = graph(p);
    let mut out = BTreeSet::new();
    let mut todo = vec![name.to_string()];
    while let Some(n) = todo.pop() {
        for (d, deps) in &g {
            if deps.contains(&n) && d != name && out.insert(d.clone()) {
                todo.push(d.clone());
            }
        }
    }
    out.into_iter().collect()
}

impl Codebase {
    pub fn find(start: &Path) -> Option<PathBuf> {
        let mut cur = Some(start);
        while let Some(d) = cur {
            if d.join(DIR).is_dir() {
                return Some(d.to_path_buf());
            }
            cur = d.parent();
        }
        None
    }

    pub fn init(root: &Path) -> Result<(), String> {
        let dir = root.join(DIR);
        if dir.exists() {
            return Err(format!("{} already exists", dir.display()));
        }
        fs::create_dir_all(dir.join("objects")).map_err(|e| e.to_string())?;
        fs::write(dir.join("index.json"), serde_json::to_string_pretty(&Index::default()).unwrap()).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn open(root: &Path) -> Result<Codebase, String> {
        let dir = root.join(DIR);
        let idx: Index = serde_json::from_str(&fs::read_to_string(dir.join("index.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let mut decls = Vec::new();
        for (_, h) in &idx.defs {
            let s = fs::read_to_string(dir.join("objects").join(format!("{h}.json"))).map_err(|e| format!("object {h}: {e}"))?;
            decls.push(serde_json::from_str(&s).map_err(|e| format!("object {h}: {e}"))?);
        }
        Ok(Codebase { root: root.to_path_buf(), program: Program { decls } })
    }

    fn dir(&self) -> PathBuf {
        self.root.join(DIR)
    }

    /// Replace the program with `new`, writing objects and a log entry.
    pub fn commit(&mut self, new: Program, message: &str) -> Result<Vec<Change>, String> {
        let old: HashMap<String, &Decl> = self.program.decls.iter().map(|d| (d.qname(), d)).collect();
        let newm: HashMap<String, &Decl> = new.decls.iter().map(|d| (d.qname(), d)).collect();
        let mut changes = Vec::new();
        for d in &new.decls {
            let n = d.qname();
            let h = self_hash(d);
            match old.get(&n) {
                Some(o) if self_hash(o) == h => {}
                Some(o) => {
                    let ic = interface_hash(o) != interface_hash(d);
                    changes.push(Change {
                        name: n.clone(),
                        kind: d.kind_str().into(),
                        old: Some(self_hash(o)),
                        new: Some(h),
                        interface_changed: ic,
                        affected: if ic { dependents(&new, &n) } else { vec![] },
                    })
                }
                None => changes.push(Change { name: n.clone(), kind: d.kind_str().into(), old: None, new: Some(h), interface_changed: true, affected: vec![] }),
            }
        }
        for d in &self.program.decls {
            if !newm.contains_key(&d.qname()) {
                changes.push(Change { name: d.qname(), kind: d.kind_str().into(), old: Some(self_hash(d)), new: None, interface_changed: true, affected: dependents(&self.program, &d.qname()) });
            }
        }
        if changes.is_empty() {
            return Ok(changes);
        }
        let objs = self.dir().join("objects");
        let mut idx = Index::default();
        for d in &new.decls {
            let h = self_hash(d);
            let p = objs.join(format!("{h}.json"));
            if !p.exists() {
                fs::write(&p, serde_json::to_string_pretty(d).unwrap()).map_err(|e| e.to_string())?;
            }
            idx.defs.push((d.qname(), h));
        }
        fs::write(self.dir().join("index.json"), serde_json::to_string_pretty(&idx).unwrap()).map_err(|e| e.to_string())?;
        let entry = serde_json::json!({
            "time": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
            "message": message,
            "changes": changes,
        });
        use std::io::Write;
        let mut f = fs::OpenOptions::new().create(true).append(true).open(self.dir().join("log.jsonl")).map_err(|e| e.to_string())?;
        writeln!(f, "{entry}").map_err(|e| e.to_string())?;
        self.program = new;
        Ok(changes)
    }

    pub fn log(&self) -> Vec<serde_json::Value> {
        fs::read_to_string(self.dir().join("log.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    pub fn load_verdicts(&self) -> HashMap<String, crate::verify::Verdict> {
        fs::read_to_string(self.dir().join("verdicts.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save_verdicts(&self, v: &HashMap<String, crate::verify::Verdict>) {
        let _ = fs::write(self.dir().join("verdicts.json"), serde_json::to_string_pretty(v).unwrap());
    }
}

/// Merge edited declarations into a program: same-named definitions are
/// replaced in place, new ones appended.
pub fn merge(base: &Program, edits: Program, delete: &[String]) -> Program {
    let mut decls: Vec<Decl> = base.decls.iter().filter(|d| !delete.contains(&d.qname())).cloned().collect();
    for e in edits.decls {
        match decls.iter_mut().find(|d| d.qname() == e.qname()) {
            Some(slot) => *slot = e,
            None => decls.push(e),
        }
    }
    Program { decls }
}
