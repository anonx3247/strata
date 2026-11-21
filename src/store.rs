//! The codebase: a package's files, plus its log. Definitions are identified
//! by the hash of their canonical form. Every change made through strata is
//! an atomic, checked patch: the files are rewritten and an entry is appended
//! to `strata.log`, which is committed with them. Entries carry the changed
//! definitions' text, so the log replays on its own (for diffs and merges).

use crate::ast::*;
use crate::printer;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use crate::fx::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The local cache folder (verification results), never committed.
pub const DIR: &str = ".strata";

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

/// A hash as shown and stored: its first 8 bytes, in hex.
fn short_hex(h: &crate::binary::Hash) -> String {
    h[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// Hash of a definition's binary encoding, comments and all (its identity in
/// the store). The hashes are those a module file's header lists.
pub fn self_hash(d: &Decl) -> String {
    short_hex(&crate::binary::hash(d))
}

/// Hash of what a definition does: without its doc and comments.
pub fn code_hash(d: &Decl) -> String {
    short_hex(&crate::binary::code_hash(d))
}

/// Hash of what other definitions may depend on: the signature, contracts,
/// type shapes and visibility; not bodies, docs or comments.
pub fn interface_hash(d: &Decl) -> String {
    short_hex(&crate::binary::interface_hash(d))
}

/// Names of other top-level definitions this one refers to.
pub fn refs(d: &Decl, ctor_types: &HashMap<String, String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    fn ty(t: &TypeExpr, out: &mut BTreeSet<String>) {
        match t {
            TypeExpr::Named(n, args) => {
                out.insert(n.to_string());
                args.iter().for_each(|a| ty(a, out));
            }
            TypeExpr::Fn(ps, r) => {
                ps.iter().for_each(|a| ty(a, out));
                ty(r, out);
            }
            TypeExpr::Unit => {}
            TypeExpr::Refined(b, _) => ty(b, out),
            TypeExpr::Union(ms) => ms.iter().for_each(|m| ty(m, out)),
        }
    }
    fn pat(p: &Pattern, out: &mut BTreeSet<String>, ct: &HashMap<String, String>) {
        match p {
            Pattern::Ctor(c, cp) => {
                out.insert(ct.get(c.as_str()).cloned().unwrap_or(c.to_string()));
                match cp {
                    CtorPat::Positional(ps) | CtorPat::Type(ps) => ps.iter().for_each(|p| pat(p, out, ct)),
                    CtorPat::Record(fs, _) => fs.iter().for_each(|(_, p)| pat(p, out, ct)),
                    CtorPat::None => {}
                }
            }
            Pattern::List(ps, _) | Pattern::Tuple(ps) | Pattern::Or(ps) => ps.iter().for_each(|p| pat(p, out, ct)),
            Pattern::Cond(c) => ex(c, out, ct),
            _ => {}
        }
    }
    fn ex(e: &Expr, out: &mut BTreeSet<String>, ct: &HashMap<String, String>) {
        let go = |x: &Expr, out: &mut BTreeSet<String>| ex(x, out, ct);
        match e {
            Expr::Var(v) => {
                out.insert(v.to_string());
            }
            Expr::List(xs) | Expr::Tuple(xs) => xs.iter().for_each(|x| go(x, out)),
            Expr::Ctor(c, args) => {
                out.insert(ct.get(c.as_str()).cloned().unwrap_or(c.to_string()));
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
            Expr::Method(r, m, xs) => {
                out.insert(m.to_string());
                go(r, out);
                xs.iter().for_each(|x| go(x, out));
            }
            Expr::Field(r, _) | Expr::Un(_, r) | Expr::Return(r) | Expr::Mut(r) | Expr::Try(r, _) => go(r, out),
            Expr::Lambda(ps, r) => {
                ps.iter().filter_map(|p| p.ty.as_ref()).for_each(|t| ty(t, out));
                go(r, out)
            }
            Expr::Except(r, _, fb) | Expr::Or(r, fb) => {
                go(r, out);
                go(fb, out);
            }
            Expr::Interp(parts) => parts.iter().for_each(|p| {
                if let StrPart::Expr(e, _) = p {
                    go(e, out)
                }
            }),
            Expr::Mock(t) => ty(t, out),
            Expr::Is(r, c) => {
                out.insert(ct.get(c.as_str()).cloned().unwrap_or(c.to_string()));
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
            Expr::Match(s, _, arms) => {
                go(s, out);
                for a in arms {
                    pat(&a.pat, out, ct);
                    go(&a.body, out);
                }
            }
            Expr::Block(stmts, last) => {
                for s in stmts {
                    match s {
                        Stmt::ComptimeFor(..) => {}
                        Stmt::Var(_, t, x) => {
                            if let Some(t) = t {
                                ty(t, out);
                            }
                            go(x, out);
                        }
                        Stmt::Assign(a, b) | Stmt::While(a, b) => {
                            go(a, out);
                            go(b, out);
                        }
                        Stmt::Loop(b) => go(b, out),
                        Stmt::Break | Stmt::Continue => {}
                        Stmt::For(p, a, b) => {
                            pat(p, out, ct);
                            go(a, out);
                            go(b, out);
                        }
                        Stmt::Let(p, t, x) => {
                            pat(p, out, ct);
                            if let Some(t) = t {
                                ty(t, out);
                            }
                            go(x, out);
                        }
                        Stmt::Expr(x) | Stmt::Assert(x) => go(x, out),
                        Stmt::Comment(_) => {}
                    }
                }
                go(last, out);
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
            out.remove(t.name.as_str());
        }
        DeclKind::Fn(f) => {
            f.bounds.iter().flat_map(|b| &b.traits).for_each(|t| ty(t, &mut out));
            f.params.iter().for_each(|p| ty(&p.ty, &mut out));
            ty(&f.ret, &mut out);
            f.requires.iter().chain(&f.ensures).for_each(|e| ex(e, &mut out, ctor_types));
            ex(&f.body, &mut out, ctor_types);
            out.remove(f.name.as_str());
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
            out.remove(t.name.as_str());
            out.remove("Self");
        }
        DeclKind::Impl(i) => {
            if !i.is_inherent() {
                out.insert(i.trait_name.to_string());
            }
            i.trait_args.iter().for_each(|a| ty(a, &mut out));
            ty(&i.for_type, &mut out);
            for m in &i.methods {
                m.params.iter().for_each(|p| ty(&p.ty, &mut out));
                ty(&m.ret, &mut out);
                ex(&m.body, &mut out, ctor_types);
            }
            out.remove("Self");
        }
        DeclKind::Test(t) => ex(&t.body, &mut out, ctor_types),
        DeclKind::Use(_) | DeclKind::Extern(ExternDecl { item: ExternItem::Type(_), .. }) => {}
        DeclKind::Extern(ExternDecl { item: ExternItem::Fn(f), .. }) => {
            f.params.iter().for_each(|p| ty(&p.ty, &mut out));
            ty(&f.ret, &mut out);
            f.requires.iter().chain(&f.ensures).for_each(|e| ex(e, &mut out, ctor_types));
            out.remove(f.name.as_str());
        }
    }
    out
}

/// Dependency graph restricted to names defined in the program.
pub fn graph(p: &Program) -> BTreeMap<String, BTreeSet<String>> {
    graph_of(p, &crate::resolve::resolve(p).program)
}

/// [`graph`], given `p` already resolved (`resolved`), so a caller that has
/// the resolution doesn't pay for it twice.
pub fn graph_of(original: &Program, resolved: &Program) -> BTreeMap<String, BTreeSet<String>> {
    // compute on resolved (qualified) names, report by each definition's qname;
    // only the program's own definitions: not the standard library resolution adds
    let p = &Program { decls: resolved.decls[..original.decls.len().min(resolved.decls.len())].to_vec() };
    let to_q: HashMap<String, String> = p.decls.iter().zip(&original.decls).map(|(r, d)| (r.name(), d.qname())).collect();
    let names: BTreeSet<String> = p.decls.iter().map(|d| d.name()).collect();
    let mut ct = HashMap::default();
    for d in &p.decls {
        if let DeclKind::Type(t) = &d.kind {
            if let TypeBody::Sum(vs) = &t.body {
                for v in vs {
                    ct.insert(v.name.to_string(), t.name.to_string());
                }
            }
        }
    }
    // a call to a trait method depends on the trait
    let mut method_trait: HashMap<Symbol, Symbol> = HashMap::default();
    for d in &p.decls {
        if let DeclKind::Trait(t) = &d.kind {
            for m in &t.methods {
                method_trait.insert(m.decl.name, t.name);
            }
        }
    }
    p.decls
        .iter()
        .map(|d| {
            let deps = refs(d, &ct)
                .into_iter()
                .map(|r| method_trait.get(&r.sym()).map(|t| t.to_string()).unwrap_or(r))
                .filter(|r| names.contains(r) && r != &d.name())
                .map(|r| to_q[&r].clone())
                .collect();
            (to_q[&d.name()].clone(), deps)
        })
        .collect()
}

/// Hash of a definition together with everything it (transitively) depends on.
/// Cached verification results are keyed by this: if nothing reachable changed,
/// the result still holds.
pub fn closure_hash(p: &Program, name: &str) -> String {
    // docs and comments don't change what a definition does, so they don't invalidate results
    let hashes: HashMap<String, String> = p.decls.iter().map(|d| (d.qname(), code_hash(d))).collect();
    closure_hash_in(&graph(p), &hashes, name)
}

/// [`closure_hash`] from the program's graph and each definition's [`code_hash`],
/// to hash many definitions without rebuilding them.
pub fn closure_hash_in(g: &BTreeMap<String, BTreeSet<String>>, hashes: &HashMap<String, String>, name: &str) -> String {
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
    /// The package a folder is in: the nearest folder at or above it with a `strata.toml`.
    pub fn find(start: &Path) -> Option<PathBuf> {
        let start = fs::canonicalize(start).unwrap_or(start.to_path_buf());
        start.ancestors().find(|d| d.join(crate::package::FILE).is_file()).map(|d| d.to_path_buf())
    }

    /// Make the current folder a package: a `strata.toml` named after the folder.
    pub fn init(root: &Path) -> Result<String, String> {
        let toml = root.join(crate::package::FILE);
        if toml.exists() {
            return Err(format!("{} already exists", toml.display()));
        }
        let text = crate::package::template(root);
        fs::write(&toml, &text).map_err(|e| e.to_string())?;
        // there from the start, so `git add` picks it up with the package
        if !root.join(LOG).exists() {
            fs::write(root.join(LOG), "").map_err(|e| e.to_string())?;
        }
        Ok(text)
    }

    /// The package's modules, read from its files.
    pub fn open(root: &Path) -> Result<Codebase, String> {
        let program = crate::package::program(root)?;
        Ok(Codebase { root: root.to_path_buf(), program })
    }

    /// The local cache (verification results); not committed.
    fn cache(&self) -> PathBuf {
        self.root.join(DIR)
    }

    /// Commit, adding `extra`'s fields to the log entry (e.g. `"rename"`).
    pub fn commit_with(&mut self, new: Program, message: &str, extra: serde_json::Value) -> Result<Vec<Change>, String> {
        let old = self.program.clone();
        let changes = changes(&old, &new);
        if changes.is_empty() {
            return Ok(changes);
        }
        self.write(new)?;
        self.log_entry(&old, message, extra)
    }

    /// Rewrite the files to `new` (one module file per module; a module left
    /// with no definitions loses its file), without logging.
    pub fn write(&mut self, new: Program) -> Result<(), String> {
        write_modules(&self.root, &self.program, &new)?;
        self.program = new;
        Ok(())
    }

    /// Append one log entry: what changed from `old` to the current program.
    pub fn log_entry(&self, old: &Program, message: &str, extra: serde_json::Value) -> Result<Vec<Change>, String> {
        let new = &self.program;
        let changes = changes(old, new);
        if changes.is_empty() {
            return Ok(changes);
        }
        let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let texts: HashMap<String, (String, String)> = new.decls.iter().map(|d| (d.qname(), (d.module.to_string(), printer::decl(d)))).collect();
        let old_texts: HashMap<String, (String, String)> = old.decls.iter().map(|d| (d.qname(), (d.module.to_string(), printer::decl(d)))).collect();
        let changes_json: Vec<serde_json::Value> = changes
            .iter()
            .map(|c| {
                let mut j = serde_json::to_value(c).unwrap();
                // the entry carries the texts before and after, so the log replays
                // either way without an object store
                if let (Some(_), Some((m, t))) = (&c.new, texts.get(&c.name)) {
                    j["module"] = serde_json::json!(m);
                    j["text"] = serde_json::json!(t);
                }
                if let (Some(_), Some((m, t))) = (&c.old, old_texts.get(&c.name)) {
                    j["module"] = serde_json::json!(m);
                    j["old_text"] = serde_json::json!(t);
                }
                j
            })
            .collect();
        let mut entry = serde_json::json!({
            "id": sha(&format!("{time}{message}{}", serde_json::to_string(&changes_json).unwrap())),
            "time": (time / 1_000_000_000) as u64,
            "message": message,
            "changes": changes_json,
        });
        if let (Some(e), Some(x)) = (entry.as_object_mut(), extra.as_object()) {
            e.extend(x.clone());
        }
        use std::io::Write;
        let mut f = fs::OpenOptions::new().create(true).append(true).open(self.root.join(LOG)).map_err(|e| e.to_string())?;
        writeln!(f, "{entry}").map_err(|e| e.to_string())?;
        Ok(changes)
    }

    /// The package's folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The local cache folder, created if needed.
    pub fn cache_dir(&self) -> PathBuf {
        let d = self.cache();
        let _ = fs::create_dir_all(&d);
        d
    }

    pub fn log(&self) -> Vec<serde_json::Value> {
        parse_log(&fs::read_to_string(self.root.join(LOG)).unwrap_or_default())
    }

    /// The program around each log entry: `[k]` is the codebase after the
    /// first `k` entries, so the last one is the files as they are. The log is
    /// replayed backwards from the files, so code written by hand before the
    /// first entry is `[0]`.
    pub fn states(&self) -> Result<Vec<Program>, String> {
        let log = self.log();
        let mut cur = self.program.clone();
        let mut out = vec![cur.clone()];
        for e in log.iter().rev() {
            undo(&mut cur, e)?;
            out.push(cur.clone());
        }
        out.reverse();
        Ok(out)
    }

    pub fn load_verdicts(&self) -> HashMap<String, crate::verify::Verdict> {
        fs::read_to_string(self.cache().join("verdicts.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save_verdicts(&self, v: &HashMap<String, crate::verify::Verdict>) {
        let _ = fs::create_dir_all(self.cache());
        let _ = fs::write(self.cache().join("verdicts.json"), serde_json::to_string_pretty(v).unwrap());
    }
}

/// Each module of `p` as a module file: (path in its package, bytes).
pub fn encode_files(p: &Program) -> Vec<(PathBuf, Vec<u8>)> {
    crate::binary::encode_modules(p).into_iter().map(|(m, bytes)| (module_path(&m), bytes)).collect()
}

/// A file that is a whole program on its own (a script): one module file
/// if it has one module (or none), else a bundle of them.
pub fn encode_script(p: &Program) -> Vec<u8> {
    let mut modules = p.decls.iter().map(|d| d.module);
    match modules.next() {
        Some(m) if modules.any(|n| n != m) => crate::binary::encode_bundle(&crate::binary::Image { manifest: None, program: p.clone() }),
        m => {
            let stored = crate::resolve::stored_form(p);
            crate::binary::encode_module(&stored, m.as_ref().map_or("", |m| m.as_str()), &graph(&stored))
        }
    }
}

/// The module a file is: in its package, named after its path in it
/// (`shop/orders.strata` is `shop.orders`, `main.strata` the root module,
/// `std/io.strata` is `std.io`); a file outside every package is a script,
/// and keeps the module it has (`None`).
pub fn module_at(path: &Path) -> Result<Option<(crate::package::Package, String)>, String> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let Some(pkg) = crate::package::find(parent)? else { return Ok(None) };
    let full = fs::canonicalize(parent).unwrap_or(parent.to_path_buf()).join(path.file_name().unwrap_or_default());
    let rel = full.strip_prefix(&pkg.dir).map_err(|_| format!("{} is not in its package's folder", path.display()))?;
    let mut m = rel.with_extension("").to_string_lossy().replace(std::path::MAIN_SEPARATOR, ".");
    if m == "main" {
        m.clear();
    }
    if pkg.name == crate::stdlib::NAME {
        m = if m.is_empty() { crate::stdlib::NAME.to_string() } else { format!("{}.{m}", crate::stdlib::NAME) };
    }
    Ok(Some((pkg, m)))
}

/// The bytes of the file at `path` holding `decls`: in a package, the module
/// file of its module, with the package's other files as they are on disk
/// (a definition's header entry lists what it uses there); else a script.
pub fn encode_at(path: &Path, mut decls: Vec<Decl>) -> Result<Vec<u8>, String> {
    let Some((pkg, m)) = module_at(path)? else { return Ok(encode_script(&Program { decls })) };
    let full = fs::canonicalize(path).unwrap_or(pkg.dir.join(module_path(&m)));
    let mut all = Vec::new();
    for (rel, bytes) in crate::package::files(&pkg.dir)? {
        if pkg.dir.join(&rel) != full {
            all.extend(crate::package::decode_file(&rel, &bytes)?.decls);
        }
    }
    for d in &mut decls {
        d.module = Symbol::from(m.as_str());
    }
    // the other modules as stored, this one as written: storing qualifies both alike
    all.extend(decls);
    let p = Program { decls: all };
    let stored = crate::resolve::stored_form(&p);
    Ok(crate::binary::encode_module(&stored, &m, &graph(&stored)))
}

/// Write `new`'s module files under `root`: only those whose bytes change;
/// the files of `old`'s modules that `new` no longer has are removed.
pub fn write_modules(root: &Path, old: &Program, new: &Program) -> Result<(), String> {
    let files = encode_files(new);
    for (rel, bytes) in &files {
        let path = root.join(rel);
        if fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            write_file(&path, bytes)?;
        }
    }
    for d in &old.decls {
        let rel = module_path(&d.module);
        let path = root.join(&rel);
        if !files.iter().any(|(r, _)| *r == rel) && path.exists() {
            fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(())
}

/// Write a file through a temporary one renamed over it, so a reader never
/// sees half of it.
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("strata-tmp");
    fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The extension of a module rendered as text (`strata render --split`, the
/// files `strata edit` opens in an editor): never read as a module file.
pub const TEXT_EXT: &str = "st";

/// The log file at a package's root, committed with the sources.
pub const LOG: &str = "strata.log";

/// Where a module's file is, relative to its package: `shop.orders` is
/// `shop/orders.strata`, the root module `main.strata`.
pub fn module_path(m: &str) -> PathBuf {
    // inside `std/` itself, `std.io` is `io.strata`
    let m = m.strip_prefix("std.").unwrap_or(if m == crate::stdlib::NAME { "" } else { m });
    if m.is_empty() { PathBuf::from("main.strata") } else { PathBuf::from(format!("{}.strata", m.replace('.', "/"))) }
}

pub fn parse_log(text: &str) -> Vec<serde_json::Value> {
    text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// What changed from `old` to `new`, definition by definition.
pub fn changes(old_p: &Program, new: &Program) -> Vec<Change> {
    let old: HashMap<String, &Decl> = old_p.decls.iter().map(|d| (d.qname(), d)).collect();
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
                    affected: if ic { dependents(new, &n) } else { vec![] },
                })
            }
            None => changes.push(Change { name: n.clone(), kind: d.kind_str().into(), old: None, new: Some(h), interface_changed: true, affected: vec![] }),
        }
    }
    for d in &old_p.decls {
        if !newm.contains_key(&d.qname()) {
            changes.push(Change { name: d.qname(), kind: d.kind_str().into(), old: Some(self_hash(d)), new: None, interface_changed: true, affected: dependents(old_p, &d.qname()) });
        }
    }
    changes
}

/// Apply one log entry to a program: a rename keeps the definition's place,
/// a changed definition is replaced by the entry's text, a removed one goes.
pub fn apply(p: &mut Program, e: &serde_json::Value) -> Result<(), String> {
    for (f, t) in renames_of(e) {
        // a renamed (or moved) definition's new text comes with the entry; keep its slot
        if let Some(i) = p.decls.iter().position(|d| d.qname() == f) {
            let (module, short) = t.rsplit_once('.').unwrap_or(("", &t));
            set_name(&mut p.decls[i], short);
            p.decls[i].module = Symbol::from(module);
        }
    }
    for c in e["changes"].as_array().into_iter().flatten() {
        let Some(name) = c["name"].as_str() else { continue };
        match (c["new"].as_str(), c["text"].as_str()) {
            (Some(_), Some(text)) => {
                let module = c["module"].as_str().unwrap_or("");
                let parsed = crate::parser::parse_module_file(text, module).map_err(|err| format!("log entry {}: {name}: {err}", e["id"]))?;
                let Some(d) = parsed.decls.into_iter().next() else { continue };
                match p.decls.iter_mut().find(|x| x.qname() == name) {
                    Some(slot) => *slot = d,
                    None => p.decls.push(d),
                }
            }
            (Some(_), None) => return Err(format!("log entry {} has no text for {name}", e["id"])),
            (None, _) => p.decls.retain(|x| x.qname() != name),
        }
    }
    // an entry's text is read in the program it changes: references as stored
    *p = crate::resolve::stored_form(p);
    Ok(())
}

/// Undo one log entry: the definitions it changed get their old text back,
/// the ones it added go, and a rename gives the definition its old name in
/// the same place.
pub fn undo(p: &mut Program, e: &serde_json::Value) -> Result<(), String> {
    let parse = |c: &serde_json::Value, text: &str| -> Result<Option<Decl>, String> {
        let module = c["module"].as_str().unwrap_or("");
        let parsed = crate::parser::parse_module_file(text, module).map_err(|err| format!("log entry {}: {}: {err}", e["id"], c["name"]))?;
        Ok(parsed.decls.into_iter().next())
    };
    let renamed = renames_of(e);
    for c in e["changes"].as_array().into_iter().flatten() {
        let Some(name) = c["name"].as_str() else { continue };
        // a renamed definition comes back under its old name, in the new one's slot
        if renamed.iter().any(|(_, t)| t == name) {
            continue;
        }
        if let Some((_, t)) = renamed.iter().find(|(f, _)| f == name) {
            if let (Some(text), Some(i)) = (c["old_text"].as_str(), p.decls.iter().position(|d| d.qname() == *t)) {
                if let Some(d) = parse(c, text)? {
                    p.decls[i] = d;
                }
            }
            continue;
        }
        match (c["old"].as_str(), c["old_text"].as_str()) {
            (Some(_), Some(text)) => {
                let Some(d) = parse(c, text)? else { continue };
                match p.decls.iter_mut().find(|x| x.qname() == name) {
                    Some(slot) => *slot = d,
                    None => p.decls.push(d),
                }
            }
            (Some(_), None) => return Err(format!("log entry {} has no old text for {name}", e["id"])),
            (None, _) => p.decls.retain(|x| x.qname() != name),
        }
    }
    *p = crate::resolve::stored_form(p);
    Ok(())
}

/// The renames (and moves) a log entry records, as (from, to) qualified names.
pub fn renames_of(e: &serde_json::Value) -> Vec<(String, String)> {
    e["renames"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| Some((r["from"].as_str()?.to_string(), r["to"].as_str()?.to_string())))
        .collect()
}

/// Give a function, type or trait a new short name.
pub fn set_name(d: &mut Decl, name: &str) {
    match &mut d.kind {
        DeclKind::Fn(f) => f.name = Symbol::from(name),
        DeclKind::Type(t) => t.name = Symbol::from(name),
        DeclKind::Trait(t) => t.name = Symbol::from(name),
        _ => {}
    }
}

/// Merge edited declarations into a program: same-named definitions are
/// replaced in place, new ones appended.
pub fn merge(base: &Program, edits: Program, delete: &[String]) -> Program {
    let mut decls: Vec<Decl> = base.decls.iter().filter(|d| !delete.contains(&d.qname())).cloned().collect();
    // the edits as stored: their references qualified, given the program they go into
    let n = edits.decls.len();
    let mut with = Program { decls: decls.iter().cloned().chain(edits.decls).collect() };
    with = crate::resolve::stored_form(&with);
    let edits = Program { decls: with.decls.split_off(with.decls.len() - n) };
    for e in edits.decls {
        match decls.iter_mut().find(|d| d.qname() == e.qname()) {
            Some(slot) => *slot = e,
            None => decls.push(e),
        }
    }
    Program { decls }
}
