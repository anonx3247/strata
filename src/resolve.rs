//! Module resolution. Every name is qualified with its module (`orders.Line`),
//! `use` imports and `pub` visibility are applied, and the result is a flat
//! program the checker and interpreter can work with directly.
//!
//! A module is used only through its public contract: other modules can see
//! its `pub` definitions (types, signatures, contracts) and nothing else.
//!
//! The output has the same declarations in the same order (a `use` stays
//! as an inert declaration), so index `i` of the result is index `i` of the
//! input.

use crate::names::{module_of, qualify, short};
use crate::ast::*;
use crate::types::Diag;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
struct Item {
    module: String,
    public: bool,
}

pub struct Resolution {
    pub program: Program,
    pub errors: Vec<Diag>,
}

pub fn resolve(p: &Program) -> Resolution {
    // Programs without modules or imports are already resolved (unless they
    // call into the host, which `run` reports). The standard library goes
    // after the program's own definitions, so index `i` of the result is
    // still index `i` of the input.
    let calls_host = || p.decls.iter().any(|d| names_in(&d.kind).iter().any(|n| crate::stdlib::is_host(n)));
    let mut r = if let Some(full) = crate::stdlib::with_std(p) {
        run(&full, None)
    } else if p.decls.iter().all(|d| d.module.is_empty() && !matches!(d.kind, DeclKind::Use(_))) && !calls_host() {
        Resolution { program: p.clone(), errors: vec![] }
    } else {
        run(p, None)
    };
    static_calls(&mut r.program);
    crate::desugar::program(&mut r.program, &mut r.errors);
    r
}

/// The name a type's own method without `self` is known by once resolved:
/// `Item.new(x)` calls the function `Item.new` (`core.Item.new` in a module).
pub fn static_name(type_q: &str, method: &str) -> String {
    format!("{type_q}.{}", short(&method))
}

/// Whether a method takes `self` (else it is called on the type: `Item.new()`).
pub fn takes_self(f: &FnDecl) -> bool {
    f.params.first().is_some_and(|p| p.name == "self")
}

/// Rewrite `Item.new(x)` (a type's own method without `self`) into a call to
/// the function `Item.new`, so everything after resolution sees a plain call.
fn static_calls(p: &mut Program) {
    let short = |m: &str| short(&m).to_string();
    let mut statics: HashSet<(String, String)> = HashSet::new();
    for d in &p.decls {
        if let DeclKind::Impl(i) = &d.kind {
            if i.is_inherent() {
                for m in i.methods.iter().filter(|m| !takes_self(m)) {
                    statics.insert((crate::types::texpr_head(&i.for_type), short(&m.name)));
                }
            }
        }
    }
    if statics.is_empty() {
        return;
    }
    for d in &mut p.decls {
        decl_exprs_mut(&mut d.kind, |e| {
            if let Expr::Method(r, m, args) = e {
                // a capitalised name parses as a constructor
                if let Expr::Var(t) | Expr::Ctor(t, CtorArgs::None) = &**r {
                    if statics.contains(&(t.clone(), short(m))) {
                        *e = Expr::Call(Box::new(Expr::Var(static_name(t, m))), std::mem::take(args));
                    }
                }
            }
        });
    }
}

/// What a qualified name is: `("function" | "type" | "trait", module)` for a
/// definition, `("variant", module)` for a constructor of a sum type,
/// `("method", module)` for a trait method, `("field", owner)` for a record
/// field (`Line.qty`, or `Pending.lines` on a record variant).
pub fn member(p: &Program, q: &str) -> Option<(&'static str, String)> {
    for d in &p.decls {
        if d.qname() == q {
            return match d.kind {
                DeclKind::Fn(_) => Some(("function", d.module.clone())),
                DeclKind::Type(_) => Some(("type", d.module.clone())),
                DeclKind::Trait(_) => Some(("trait", d.module.clone())),
                _ => None,
            };
        }
        match &d.kind {
            DeclKind::Type(t) => {
                if let TypeBody::Sum(vs) = &t.body {
                    if vs.iter().any(|v| qualify(&d.module, &v.name) == q) {
                        return Some(("variant", d.module.clone()));
                    }
                }
            }
            DeclKind::Trait(t) => {
                if t.methods.iter().any(|m| qualify(&d.module, &m.decl.name) == q) {
                    return Some(("method", d.module.clone()));
                }
            }
            _ => {}
        }
    }
    let (owner, field) = q.rsplit_once('.')?;
    fields_of(p, owner).iter().any(|f| f == field).then(|| ("field", owner.to_string()))
}

/// The definition that declares `q`: itself, the type of a variant or field,
/// or the trait of a method.
pub fn owner(p: &Program, q: &str) -> Option<String> {
    let (kind, of) = member(p, q)?;
    let target = if kind == "field" { of } else { q.to_string() };
    p.decls.iter().find(|d| {
        d.qname() == target
            || match &d.kind {
                DeclKind::Type(t) => matches!(&t.body, TypeBody::Sum(vs) if vs.iter().any(|v| qualify(&d.module, &v.name) == target)),
                DeclKind::Trait(t) => t.methods.iter().any(|m| qualify(&d.module, &m.decl.name) == target),
                _ => false,
            }
    })
    .map(|d| d.qname())
}

/// Give the definition, variant, trait method (with its impls' methods) or
/// field `q` the short name `new`, without touching anything that refers to it.
pub fn set_member_name(p: &mut Program, q: &str, new: &str) {
    let Some((kind, of)) = member(p, q) else { return };
    let short = short(q).to_string();
    for d in &mut p.decls {
        let m = d.module.clone();
        if d.qname() == q {
            match &mut d.kind {
                DeclKind::Fn(f) => f.name = new.to_string(),
                DeclKind::Type(t) => t.name = new.to_string(),
                DeclKind::Trait(t) => t.name = new.to_string(),
                _ => {}
            }
            continue;
        }
        match &mut d.kind {
            DeclKind::Type(t) => {
                let tq = qualify(&m, &t.name);
                match &mut t.body {
                    TypeBody::Record(fs) if kind == "field" && tq == of => fs.iter_mut().filter(|f| f.name == short).for_each(|f| f.name = new.to_string()),
                    TypeBody::Sum(vs) => {
                        for v in vs.iter_mut() {
                            let vq = qualify(&m, &v.name);
                            if kind == "variant" && vq == q {
                                v.name = new.to_string();
                            }
                            if let (Payload::Record(fs), true) = (&mut v.payload, kind == "field" && vq == of) {
                                fs.iter_mut().filter(|f| f.name == short).for_each(|f| f.name = new.to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
            DeclKind::Trait(t) if kind == "method" => {
                for tm in t.methods.iter_mut().filter(|tm| qualify(&m, &tm.decl.name) == q) {
                    tm.decl.name = new.to_string();
                }
            }
            // an impl of the method's trait (method names are unique across traits)
            DeclKind::Impl(i) if kind == "method" => {
                for f in i.methods.iter_mut().filter(|f| f.name == short) {
                    f.name = new.to_string();
                }
            }
            _ => {}
        }
    }
}

/// The fields of a record type or a record variant, by qualified name.
fn fields_of(p: &Program, owner: &str) -> Vec<String> {
    for d in &p.decls {
        let DeclKind::Type(t) = &d.kind else { continue };
        match &t.body {
            TypeBody::Record(fs) if qualify(&d.module, &t.name) == owner => return fs.iter().map(|f| f.name.clone()).collect(),
            TypeBody::Sum(vs) => {
                for v in vs {
                    if let Payload::Record(fs) = &v.payload {
                        if qualify(&d.module, &v.name) == owner {
                            return fs.iter().map(|f| f.name.clone()).collect();
                        }
                    }
                }
            }
            _ => {}
        }
    }
    vec![]
}

/// Rename fields, all at once: each `(owner, field, new)` renames field
/// `field` of `owner` (a record type or record variant) to `new`: the
/// declaration, accesses (`x.qty`), and literals, updates and patterns of the
/// owner's constructor. Accesses are told apart by type, not by name: when
/// several records have a field of that name, the checker says which records
/// each access reads from (see `types::field_owners`). An access is renamed
/// when every record it reads from is renamed, to the same name; one that
/// mixes renamed and other records is refused. The caller type-checks the
/// result.
pub fn rename_fields(p: &Program, renames: &[(String, String, String)]) -> Result<Program, String> {
    for (owner, field, new) in renames {
        if fields_of(p, owner).iter().any(|f| f == new) {
            return Err(format!("`{owner}` already has a field `{new}`"));
        }
        if !fields_of(p, owner).iter().any(|f| f == field) {
            return Err(format!("`{owner}` has no field `{field}`"));
        }
    }
    let mut fields: Vec<&str> = renames.iter().map(|r| r.1.as_str()).collect();
    fields.sort();
    fields.dedup();
    // records with a field of that name besides those renamed (or `.value` of distinct
    // types): then accesses are told apart by the checker's types
    let prelude = crate::types::prelude();
    let records: Vec<(String, Vec<String>)> = p
        .decls
        .iter()
        .chain(&prelude.decls)
        .flat_map(|d| match &d.kind {
            DeclKind::Type(t) => match &t.body {
                TypeBody::Record(fs) => vec![(qualify(&d.module, &t.name), fs.iter().map(|f| f.name.clone()).collect())],
                TypeBody::Sum(vs) => vs
                    .iter()
                    .filter_map(|v| match &v.payload {
                        Payload::Record(fs) => Some((qualify(&d.module, &v.name), fs.iter().map(|f| f.name.clone()).collect())),
                        _ => None,
                    })
                    .collect(),
                _ => vec![],
            },
            _ => vec![],
        })
        .collect();
    let distinct = p.decls.iter().any(|d| matches!(&d.kind, DeclKind::Type(t) if t.distinct));
    let typed = fields.iter().any(|f| {
        let renamed: Vec<&str> = renames.iter().filter(|r| r.1 == *f).map(|r| r.2.as_str()).collect();
        let same_new = renamed.windows(2).all(|w| w[0] == w[1]);
        let others = records.iter().any(|(q, fs)| fs.iter().any(|x| x == f) && !renames.iter().any(|r| r.0 == *q && r.1 == *f));
        others || !same_new || (*f == "value" && distinct)
    });
    let mut modules: HashSet<String> = HashSet::new();
    for d in &p.decls {
        let mut m = d.module.as_str();
        while !m.is_empty() {
            modules.insert(m.to_string());
            m = module_of(m);
        }
    }
    // which records each access reads from, from the checker, when it matters
    let owners = if typed {
        let mut tagged = p.clone();
        for (i, d) in tagged.decls.iter_mut().enumerate() {
            for f in &fields {
                let mut k = 0;
                let mut tag = |_: &str| -> Option<String> {
                    k += 1;
                    Some(format!("{f}·{f}/d{i}k{}", k - 1))
                };
                field_uses(&mut d.kind, f, "", None, &modules, &mut tag);
            }
        }
        Some(crate::types::field_owners(&tagged))
    } else {
        None
    };

    let mut out = p.clone();
    let mut problems: Vec<String> = Vec::new();
    for (i, d) in out.decls.iter_mut().enumerate() {
        let name = d.qname();
        // accesses, field by field, in the same order as the tags
        for f in &fields {
            let mut k = 0;
            let mut decide = |_: &str| -> Option<String> {
                let tag = format!("{f}/d{i}k{k}");
                k += 1;
                let Some(owners) = &owners else {
                    return renames.iter().find(|r| r.1 == *f).map(|r| r.2.clone());
                };
                let s = owners.get(&tag).cloned().unwrap_or_default();
                let news: Vec<&str> = s.iter().filter_map(|o| renames.iter().find(|r| r.0 == *o && r.1 == *f).map(|r| r.2.as_str())).collect();
                if s.is_empty() {
                    problems.push(format!("in `{name}`, the checker can't tell which record `.{f}` is read from"));
                    None
                } else if news.is_empty() {
                    None
                } else if news.len() < s.len() {
                    let rest: Vec<String> = s.iter().filter(|o| !renames.iter().any(|r| r.0 == **o && r.1 == *f)).map(|o| format!("`{o}.{f}`")).collect();
                    problems.push(format!("in `{name}`, `.{f}` is read from a union that also has {} (rename {} too, in the same command)", rest.join(", "), if rest.len() == 1 { "it" } else { "them" }));
                    None
                } else if news.windows(2).any(|w| w[0] != w[1]) {
                    problems.push(format!("in `{name}`, `.{f}` is read from a union whose records would get different names"));
                    None
                } else {
                    Some(news[0].to_string())
                }
            };
            field_uses(&mut d.kind, f, "", None, &modules, &mut decide);
        }
        // literals, updates and patterns, by constructor
        for (owner, f, new) in renames {
            let short = short(&owner);
            field_uses(&mut d.kind, f, short, Some(new), &modules, &mut |_| None);
        }
        // the declarations (after their uses, which start from the original)
        if let DeclKind::Type(t) = &mut d.kind {
            let tq = qualify(&d.module, &t.name);
            for (owner, f, new) in renames {
                match &mut t.body {
                    TypeBody::Record(fs) if tq == *owner => fs.iter_mut().filter(|x| x.name == *f).for_each(|x| x.name = new.clone()),
                    TypeBody::Sum(vs) => {
                        for v in vs.iter_mut().filter(|v| qualify(&d.module, &v.name) == *owner) {
                            if let Payload::Record(fs) = &mut v.payload {
                                fs.iter_mut().filter(|x| x.name == *f).for_each(|x| x.name = new.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if !problems.is_empty() {
        problems.dedup();
        return Err(format!("can't rename {}: {}", renames.iter().map(|r| format!("`{}.{}`", r.0, r.1)).collect::<Vec<_>>().join(", "), problems.join("; ")));
    }
    Ok(out)
}

/// Walk a definition in a fixed order (as written). Every field access
/// `x.field` (not a qualified name like `orders.field`) is passed to `access`,
/// which gives its new name (or `None` to leave it). With `ctor_new`, the
/// field is also renamed in literals, updates and patterns of the
/// constructor `ctor` (the owner's short name).
fn field_uses(d: &mut DeclKind, field: &str, ctor: &str, ctor_new: Option<&str>, modules: &HashSet<String>, access: &mut dyn FnMut(&str) -> Option<String>) {
    struct Uses<'a> {
        field: &'a str,
        ctor: &'a str,
        ctor_new: Option<&'a str>,
        modules: &'a HashSet<String>,
        access: &'a mut dyn FnMut(&str) -> Option<String>,
    }
    impl Uses<'_> {
        /// The fields of a `ctor` literal, update or pattern, renamed.
        fn rename<T>(&self, c: &str, fs: &mut [(String, T)]) {
            if let Some(new) = self.ctor_new.filter(|_| short(c) == self.ctor) {
                fs.iter_mut().filter(|(n, _)| n == self.field).for_each(|(n, _)| *n = new.to_string());
            }
        }
    }
    impl VisitMut for Uses<'_> {
        fn expr(&mut self, e: &mut Expr) {
            match e {
                // `x.qty`, but not `orders.qty` (a qualified name)
                Expr::Field(r, f) if f == self.field && !place_path(r).is_some_and(|m| self.modules.contains(&m)) => {
                    if let Some(n) = (self.access)(self.field) {
                        *f = n;
                    }
                }
                // `Line { qty: 1 }`, `Line { ..l, qty: 2 }`
                Expr::Ctor(c, CtorArgs::Record(fs) | CtorArgs::Update(_, fs)) => self.rename(c, fs),
                _ => {}
            }
            walk_expr_mut(self, e)
        }
        fn pattern(&mut self, p: &mut Pattern) {
            // `Line { qty, .. }`
            if let Pattern::Ctor(c, CtorPat::Record(fs, _)) = p {
                self.rename(c, fs);
            }
            walk_pattern_mut(self, p)
        }
    }
    walk_decl_mut(&mut Uses { field, ctor, ctor_new, modules, access }, d)
}

/// Several renames in one step: `(old, new)` pairs, as for `rename`. Fields
/// are renamed together (so an access on a union of renamed records can
/// follow), then the rest one after the other.
pub fn rename_all(p: &Program, pairs: &[(String, String)]) -> Result<Program, String> {
    let mut fields = Vec::new();
    let mut rest = Vec::new();
    for (old, new) in pairs {
        match member(p, old) {
            Some(("field", owner)) => {
                let short = short(&old).to_string();
                let upper = |s: &str| s.chars().next().is_some_and(|c| c.is_uppercase());
                if upper(&short) != upper(new) || !new.chars().all(|c| c.is_alphanumeric() || c == '_') || new.is_empty() {
                    return Err(format!("`{new}` is not a field name like `{short}`"));
                }
                fields.push((owner, short, new.clone()));
            }
            Some(_) => rest.push((old.clone(), new.clone())),
            None => return Err(format!("no definition, variant, trait method or field `{old}`")),
        }
    }
    let mut out = if fields.is_empty() { p.clone() } else { rename_fields(p, &fields)? };
    for (old, new) in rest {
        out = rename(&out, &old, &new)?;
    }
    Ok(out)
}

/// Every qualified name the program defines: definitions, variants, trait methods.
fn defined(p: &Program) -> HashSet<String> {
    let mut out = HashSet::new();
    for d in &p.decls {
        out.insert(d.qname());
        match &d.kind {
            DeclKind::Type(t) => {
                if let TypeBody::Sum(vs) = &t.body {
                    out.extend(vs.iter().map(|v| qualify(&d.module, &v.name)));
                }
            }
            DeclKind::Trait(t) => out.extend(t.methods.iter().map(|m| qualify(&d.module, &m.decl.name))),
            _ => {}
        }
    }
    out
}

/// Rename `old` (a qualified name: a function, type, trait, variant, trait
/// method, or a field as `Owner.field`) to `new` (a short name), and every
/// reference to it. Names are looked up exactly as the checker does (locals
/// shadow globals, `use` brings names in), and everything else is written
/// back as it was.
pub fn rename(p: &Program, old: &str, new: &str) -> Result<Program, String> {
    let (kind, module) = member(p, old).ok_or(format!("no definition, variant, trait method or field `{old}`"))?;
    let valid = new.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && new.chars().all(|c| c.is_alphanumeric() || c == '_');
    if !valid {
        return Err(format!("`{new}` is not a name (give the new short name, without the module)"));
    }
    let short = short(&old);
    let upper = |s: &str| s.chars().next().is_some_and(|c| c.is_uppercase());
    if upper(short) != upper(new) {
        return Err(format!("`{new}` must start with {} like `{short}`", if upper(new) { "a lowercase letter" } else { "an uppercase letter" }));
    }
    if kind == "field" {
        return rename_fields(p, &[(module, short.to_string(), new.to_string())]);
    }
    let new_q = qualify(&module, new);
    if defined(p).contains(&new_q) {
        return Err(format!("`{new_q}` already exists"));
    }
    let full = crate::stdlib::with_std(p);
    let out = run(full.as_ref().unwrap_or(p), Some((old.to_string(), new.to_string())));
    let mut program = out.program;
    program.decls.truncate(p.decls.len());
    for (r, d) in program.decls.iter_mut().zip(&p.decls) {
        r.module = d.module.clone();
    }
    // the rename must not change what any other name refers to
    let map = HashMap::from([(old.to_string(), new_q.clone())]);
    let expect: Vec<Decl> = resolved_defs(p).iter().map(|d| renamed(d, &map)).collect();
    if resolved_defs(&program) != expect {
        return Err(format!("renaming `{old}` to `{new}` would change what other names refer to (something visible there is already called `{new}`)"));
    }
    Ok(program)
}

/// After a merge: `merged` has the renamed definition (`new`) but also
/// references to its old name `old`. `back` is `merged` with that definition
/// under its old name again. Rename the references to `old` in `back`, and
/// check, definition by definition, that what changed resolves as before
/// (with `old` now `new`) and everything else as in `merged`.
pub fn follow(merged: &Program, back: &Program, old: &str, new: &str) -> Result<Program, String> {
    let out = run(back, Some((old.to_string(), new.to_string())));
    let mut program = out.program;
    for (r, d) in program.decls.iter_mut().zip(&back.decls) {
        r.module = d.module.clone();
    }
    let module = crate::names::module_of(&old);
    let new_q = if module.is_empty() { new.to_string() } else { format!("{module}.{new}") };
    let each = |p: &Program| resolve(p).program.decls;
    let (rm, rb, ra) = (each(merged), each(back), each(&program));
    let map = HashMap::from([(old.to_string(), new_q.clone())]);
    for (i, d) in program.decls.iter().enumerate() {
        if matches!(d.kind, DeclKind::Use(_)) {
            continue;
        }
        // the renamed definition, and what the rename didn't touch, are as merged
        let touched = program.decls[i] != back.decls[i] && back.decls[i].qname() != old;
        let expect = if touched { renamed(&rb[i], &map) } else { rm[i].clone() };
        if ra[i] != expect {
            return Err(format!("following the rename of `{old}` to `{new}` would change what `{}` refers to", d.qname()));
        }
    }
    Ok(program)
}

/// Apply `f` to every name in a definition that can refer to another
/// definition (variables and calls, constructors, methods, `is` tests,
/// types, traits, `use`s), and to the names definitions are declared under.
/// Not string literals, fields, local bindings, docs or comments.
fn names_mut(d: &mut DeclKind, f: &mut dyn FnMut(&mut String)) {
    struct Names<'a>(&'a mut dyn FnMut(&mut String));
    impl VisitMut for Names<'_> {
        fn expr(&mut self, e: &mut Expr) {
            if let Expr::Var(n) | Expr::Ctor(n, _) | Expr::Method(_, n, _) | Expr::Is(_, n) = e {
                (self.0)(n)
            }
            walk_expr_mut(self, e)
        }
        fn pattern(&mut self, p: &mut Pattern) {
            if let Pattern::Ctor(n, _) = p {
                (self.0)(n)
            }
            walk_pattern_mut(self, p)
        }
        fn ty(&mut self, t: &mut TypeExpr) {
            if let TypeExpr::Named(n, _) = t {
                (self.0)(n)
            }
            walk_ty_mut(self, t)
        }
    }
    walk_decl_mut(&mut Names(&mut *f), d);
    match d {
        DeclKind::Type(t) => {
            f(&mut t.name);
            if let TypeBody::Sum(vs) = &mut t.body {
                vs.iter_mut().for_each(|v| f(&mut v.name));
            }
        }
        DeclKind::Fn(fd) => f(&mut fd.name),
        DeclKind::Trait(t) => {
            f(&mut t.name);
            t.methods.iter_mut().for_each(|m| f(&mut m.decl.name));
        }
        DeclKind::Impl(i) => {
            if !i.trait_name.is_empty() {
                f(&mut i.trait_name);
            }
            i.methods.iter_mut().for_each(|m| f(&mut m.name));
        }
        DeclKind::Use(u) => {
            f(&mut u.path);
            u.items.iter_mut().flatten().for_each(|n| f(n));
        }
        DeclKind::Test(_) => {}
    }
}

/// The names in a definition, as `names_mut` visits them.
fn names_in(d: &DeclKind) -> HashSet<String> {
    let mut out = HashSet::new();
    names_mut(&mut d.clone(), &mut |n| {
        out.insert(n.clone());
    });
    out
}

/// A definition with names replaced: `renames` maps old qualified names to new.
fn renamed(d: &Decl, renames: &HashMap<String, String>) -> Decl {
    let mut d = d.clone();
    names_mut(&mut d.kind, &mut |n| {
        if let Some(new) = renames.get(n.as_str()) {
            *n = new.clone();
        }
    });
    d
}

/// A program resolved, without its `use`s (they are compared through the
/// names they bring in, which resolution already covers).
fn resolved_defs(p: &Program) -> Vec<Decl> {
    resolve(p).program.decls.into_iter().filter(|d| !matches!(d.kind, DeclKind::Use(_))).collect()
}

fn run(p: &Program, rename: Option<(String, String)>) -> Resolution {
    run_with(p, rename, None).0
}

/// Resolve, or write back (with a rename, or with `moves`: old qualified
/// name → new qualified name). Also gives, per definition, the moved names
/// it refers to by their short name from another module than theirs, which
/// that module must then import.
fn run_with(p: &Program, rename: Option<(String, String)>, moves: Option<&HashMap<String, String>>) -> (Resolution, Vec<Vec<String>>) {
    let mut table: HashMap<String, Item> = HashMap::new();
    // children of a type (its constructors) or a trait (its methods), as (short, qualified)
    let mut children: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut modules: HashSet<String> = HashSet::new();
    // qualified names of trait methods
    let mut methods: HashSet<String> = HashSet::new();
    for d in &p.decls {
        let mut m = d.module.as_str();
        while !m.is_empty() {
            modules.insert(m.to_string());
            m = module_of(m);
        }
        let item = Item { module: d.module.clone(), public: d.public };
        match &d.kind {
            DeclKind::Type(t) => {
                let q = qualify(&d.module, &t.name);
                table.insert(q.clone(), item.clone());
                if let TypeBody::Sum(vs) = &t.body {
                    for v in vs {
                        let cq = qualify(&d.module, &v.name);
                        table.insert(cq.clone(), item.clone());
                        children.entry(q.clone()).or_default().push((v.name.clone(), cq));
                    }
                }
            }
            DeclKind::Trait(t) => {
                let q = qualify(&d.module, &t.name);
                table.insert(q.clone(), item.clone());
                for m in &t.methods {
                    let mq = qualify(&d.module, &m.decl.name);
                    table.insert(mq.clone(), item.clone());
                    methods.insert(mq.clone());
                    children.entry(q.clone()).or_default().push((m.decl.name.clone(), mq));
                }
            }
            DeclKind::Fn(_) => {
                table.insert(qualify(&d.module, &d.name()), item);
            }
            // a type's own methods come with the type, like a trait's
            DeclKind::Impl(i) if i.is_inherent() => {
                let tq = qualify(&d.module, &crate::types::texpr_head(&i.for_type));
                for m in &i.methods {
                    let mq = qualify(&d.module, &m.name);
                    table.insert(mq.clone(), Item { module: d.module.clone(), public: true });
                    children.entry(tq.clone()).or_default().push((m.name.clone(), mq));
                }
            }
            DeclKind::Test(_) | DeclKind::Impl(_) | DeclKind::Use(_) => {}
        }
    }

    // already-qualified names (a resolved program) imply their modules too
    for q in table.keys().cloned().collect::<Vec<_>>() {
        let mut m = module_of(&q);
        while !m.is_empty() {
            modules.insert(m.to_string());
            m = module_of(m);
        }
    }
    let mut errors = Vec::new();
    // what each module can refer to by short name
    let mut scopes: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut all_modules: Vec<String> = p.decls.iter().map(|d| d.module.clone()).collect();
    all_modules.sort();
    all_modules.dedup();
    for m in &all_modules {
        let mut scope: HashMap<String, String> = HashMap::new();
        // the root module is visible everywhere
        for (q, it) in &table {
            if it.module.is_empty() {
                scope.insert(q.clone(), q.clone());
            }
        }
        for d in p.decls.iter().filter(|d| &d.module == m) {
            let DeclKind::Use(u) = &d.kind else { continue };
            if !modules.contains(&u.path) {
                errors.push(Diag { decl: d.qname(), msg: format!("there is no module `{}`", u.path) });
                continue;
            }
            match &u.items {
                None => {
                    for (q, it) in &table {
                        if it.module == u.path && it.public {
                            scope.insert(q[u.path.len() + 1..].to_string(), q.clone());
                        }
                    }
                }
                Some(items) => {
                    for i in items {
                        let q = format!("{}.{i}", u.path);
                        match table.get(&q) {
                            None => errors.push(Diag { decl: d.qname(), msg: format!("module `{}` has no `{i}`", u.path) }),
                            Some(it) if !it.public => errors.push(Diag {
                                decl: d.qname(),
                                msg: format!("`{i}` is private to module `{}` (only `pub` definitions are part of its contract)", u.path),
                            }),
                            Some(_) => {
                                scope.insert(i.clone(), q.clone());
                                // importing a type brings its constructors; a trait, its methods
                                for (short, cq) in children.get(&q).into_iter().flatten() {
                                    scope.insert(short.clone(), cq.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
        // the module's own definitions win
        for (q, it) in &table {
            if &it.module == m {
                scope.insert(q[if m.is_empty() { 0 } else { m.len() + 1 }..].to_string(), q.clone());
            }
        }
        scopes.insert(m.clone(), scope);
    }

    let imports: HashMap<String, Vec<String>> = all_modules
        .iter()
        .map(|m| (m.clone(), p.decls.iter().filter(|d| &d.module == m).filter_map(|d| if let DeclKind::Use(u) = &d.kind { Some(u.path.clone()) } else { None }).collect()))
        .collect();
    let mut decls = Vec::new();
    let mut needs = Vec::new();
    for d in &p.decls {
        let mut r = Rw { scope: &scopes[&d.module], table: &table, methods: &methods, modules: &modules, module: &d.module, imports: &imports[&d.module], decl: d.qname(), locals: vec![], tparams: vec![], errors: vec![], rename: rename.as_ref(), moves, needs: Default::default() };
        let kind = match (&d.kind, rename.as_ref()) {
            // `use orders.{ship}` no longer names a moved definition (the module imports it anew)
            (DeclKind::Use(u), _) if moves.is_some() => DeclKind::Use(UseDecl {
                path: u.path.clone(),
                items: u.items.as_ref().map(|is| is.iter().filter(|i| !moves.unwrap().contains_key(&format!("{}.{i}", u.path))).cloned().collect()),
            }),
            // `use core.{same_ids}` names what it imports
            (DeclKind::Use(u), Some((old, new))) => DeclKind::Use(UseDecl {
                path: u.path.clone(),
                items: u.items.as_ref().map(|is| is.iter().map(|i| if &format!("{}.{i}", u.path) == old { new.clone() } else { i.clone() }).collect()),
            }),
            _ => r.decl(&d.kind),
        };
        errors.extend(r.errors);
        needs.push(r.needs.into_inner());
        decls.push(Decl { comments: d.comments.clone(), doc: d.doc.clone(), kind, module: String::new(), public: d.public });
    }
    (Resolution { program: Program { decls }, errors }, needs)
}

/// `shop.orders` as an expression (a variable and field accesses).
fn path_expr(m: &str) -> Expr {
    let mut parts = m.split('.');
    let mut e = Expr::Var(parts.next().unwrap_or("").to_string());
    for p in parts {
        e = Expr::Field(Box::new(e), p.to_string());
    }
    e
}

/// Builtins that a plain call reaches even when an imported trait has a
/// method of the same name.
const PRELUDE_FNS: &[&str] = &["print"];

struct Rw<'a> {
    scope: &'a HashMap<String, String>,
    table: &'a HashMap<String, Item>,
    /// qualified names of trait methods
    methods: &'a HashSet<String>,
    modules: &'a HashSet<String>,
    module: &'a str,
    /// modules this one `use`s, for "that's private" hints
    imports: &'a [String],
    decl: String,
    locals: Vec<HashSet<String>>,
    tparams: Vec<HashSet<String>>,
    errors: Vec<Diag>,
    /// Rename mode: (old qualified name, new short name). Names are then
    /// written back as they were, except those that resolve to `old`.
    rename: Option<&'a (String, String)>,
    /// Move mode: old qualified name → new qualified name. Names are written
    /// back as they were; a qualified one that refers to a moved definition
    /// gets its new module, and a short one stays short (see `needs`).
    moves: Option<&'a HashMap<String, String>>,
    /// moved names this definition writes short, from a module that must import them
    needs: std::cell::RefCell<Vec<String>>,
}

impl Rw<'_> {
    fn is_local(&self, x: &str) -> bool {
        self.locals.iter().any(|s| s.contains(x))
    }
    fn bind(&mut self, x: &str) {
        if let Some(s) = self.locals.last_mut() {
            s.insert(x.to_string());
        }
    }
    fn push(&mut self, names: &[String]) {
        self.locals.push(names.iter().cloned().collect());
    }
    fn pop(&mut self) {
        self.locals.pop();
    }

    /// Resolve a (possibly qualified) global name; in rename mode, give it
    /// back as written, renamed if it refers to the renamed definition.
    fn name(&mut self, n: &str) -> String {
        let q = self.lookup(n);
        self.written(n, &q)
    }
    /// Writing names back (rename or move), rather than resolving them.
    fn writing(&self) -> bool {
        self.rename.is_some() || self.moves.is_some()
    }
    fn written(&self, n: &str, q: &str) -> String {
        if let Some(nq) = self.moves.and_then(|m| m.get(q)) {
            if n.contains('.') {
                return nq.clone();
            }
            if module_of(nq) != self.module {
                self.needs.borrow_mut().push(nq.clone());
            }
            return n.to_string();
        }
        match self.rename {
            None if self.moves.is_none() => q.to_string(),
            None => n.to_string(),
            Some((old, new)) if q == old => match n.rsplit_once('.') {
                Some((path, _)) => format!("{path}.{new}"),
                None => new.clone(),
            },
            Some(_) => n.to_string(),
        }
    }
    /// The name of a definition being declared, `name` in module `m`.
    fn declared(&self, m: &str, name: &str) -> String {
        self.written(name, &qualify(m, name))
    }
    fn lookup(&mut self, n: &str) -> String {
        if let Some(q) = self.scope.get(n) {
            return q.clone();
        }
        if !n.contains('.') {
            for m in self.imports {
                let q = format!("{m}.{n}");
                if self.table.get(&q).is_some_and(|it| !it.public) {
                    self.errors.push(Diag {
                        decl: self.decl.clone(),
                        msg: format!("`{n}` is private to module `{m}` (only `pub` definitions are part of its contract)"),
                    });
                    return q;
                }
            }
        }
        if let Some(it) = self.table.get(n) {
            let visible = it.public || it.module == self.module || self.module.starts_with(&format!("{}.", it.module));
            if !visible {
                self.errors.push(Diag {
                    decl: self.decl.clone(),
                    msg: format!("`{n}` is private to module `{}` (only `pub` definitions are part of its contract)", it.module),
                });
            }
        }
        if crate::stdlib::is_host(n) && !crate::stdlib::is_std(self.module) && self.rename.is_none() {
            self.errors.push(Diag {
                decl: self.decl.clone(),
                msg: format!("`{n}` is internal to the standard library: take a capability instead (`Console` from `std.io`, `Files` from `std.fs`, `Environment` from `std.process`)"),
            });
        }
        n.to_string()
    }

    /// `orders` or `shop.orders` written as variables and field accesses.
    fn module_path(&self, e: &Expr) -> Option<String> {
        let path = match e {
            Expr::Var(x) if !self.is_local(x) => x.clone(),
            Expr::Field(r, f) => format!("{}.{f}", self.module_path_any(r)?),
            _ => return None,
        };
        self.modules.contains(&path).then_some(path)
    }
    fn module_path_any(&self, e: &Expr) -> Option<String> {
        match e {
            Expr::Var(x) if !self.is_local(x) => Some(x.clone()),
            Expr::Field(r, f) => Some(format!("{}.{f}", self.module_path_any(r)?)),
            _ => None,
        }
    }

    fn ty(&mut self, t: &TypeExpr) -> TypeExpr {
        match t {
            TypeExpr::Named(n, args) => {
                let n2 = if n == "Self" || self.tparams.iter().any(|s| s.contains(n)) { n.clone() } else { self.name(n) };
                TypeExpr::Named(n2, args.iter().map(|a| self.ty(a)).collect())
            }
            TypeExpr::Fn(ps, r) => TypeExpr::Fn(ps.iter().map(|a| self.ty(a)).collect(), Box::new(self.ty(r))),
            TypeExpr::Unit => TypeExpr::Unit,
            TypeExpr::Union(ms) => TypeExpr::Union(ms.iter().map(|m| self.ty(m)).collect()),
            TypeExpr::Refined(b, p) => {
                let b = self.ty(b);
                self.push(&["it".into()]);
                let p = self.expr(p);
                self.pop();
                TypeExpr::Refined(Box::new(b), Box::new(p))
            }
        }
    }

    fn pat(&mut self, p: &Pattern) -> Pattern {
        match p {
            Pattern::Bind(x) => {
                self.bind(x);
                p.clone()
            }
            Pattern::Ctor(c, cp) => {
                let c = self.name(c);
                let cp = match cp {
                    CtorPat::None => CtorPat::None,
                    CtorPat::Positional(ps) => CtorPat::Positional(ps.iter().map(|x| self.pat(x)).collect()),
                    CtorPat::Record(fs, open) => CtorPat::Record(fs.iter().map(|(f, x)| (f.clone(), self.pat(x))).collect(), *open),
                };
                Pattern::Ctor(c, cp)
            }
            Pattern::List(items, rest) => {
                let items = items.iter().map(|x| self.pat(x)).collect();
                if let Some(r) = rest {
                    self.bind(r);
                }
                Pattern::List(items, rest.clone())
            }
            Pattern::Tuple(ps) => Pattern::Tuple(ps.iter().map(|x| self.pat(x)).collect()),
            Pattern::Or(ps) => Pattern::Or(ps.iter().map(|x| self.pat(x)).collect()),
            Pattern::Cond(c) => {
                self.push(&["it".into()]);
                let c = self.expr(c);
                self.pop();
                Pattern::Cond(Box::new(c))
            }
            other => other.clone(),
        }
    }

    fn exprs(&mut self, es: &[Expr]) -> Vec<Expr> {
        es.iter().map(|e| self.expr(e)).collect()
    }

    fn expr(&mut self, e: &Expr) -> Expr {
        match e {
            Expr::Var(x) if self.is_local(x) => e.clone(),
            Expr::Var(x) => Expr::Var(self.name(x)),
            Expr::List(es) => Expr::List(self.exprs(es)),
            Expr::Tuple(es) => Expr::Tuple(self.exprs(es)),
            Expr::Ctor(c, args) => {
                let c = self.name(c);
                let args = match args {
                    CtorArgs::None => CtorArgs::None,
                    CtorArgs::Positional(es) => CtorArgs::Positional(self.exprs(es)),
                    CtorArgs::Record(fs) => CtorArgs::Record(fs.iter().map(|(f, x)| (f.clone(), self.expr(x))).collect()),
                    CtorArgs::Update(b, fs) => CtorArgs::Update(Box::new(self.expr(b)), fs.iter().map(|(f, x)| (f.clone(), self.expr(x))).collect()),
                };
                Expr::Ctor(c, args)
            }
            // `print(x)` is the prelude's, even where a trait with a `print`
            // method is imported (`console.print(x)` is the method)
            Expr::Call(f, args) if matches!(&**f, Expr::Var(x) if PRELUDE_FNS.contains(&x.as_str()) && !self.is_local(x) && self.scope.get(x).is_none_or(|q| self.methods.contains(q))) => {
                Expr::Call(f.clone(), self.exprs(args))
            }
            Expr::Call(f, args) => Expr::Call(Box::new(self.expr(f)), self.exprs(args)),
            Expr::Method(recv, m, args) => {
                if let Some(path) = self.module_path(recv) {
                    // `orders.ship(x)`: a qualified call, not a method
                    let f = self.name(&format!("{path}.{m}"));
                    if self.writing() {
                        // `orders.ship(x)`: a move changes the module written before it
                        let (fp, m) = f.rsplit_once('.').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((path.clone(), m.clone()));
                        let recv = if fp == path { recv.clone() } else { Box::new(path_expr(&fp)) };
                        return Expr::Method(recv, m, self.exprs(args));
                    }
                    return Expr::Call(Box::new(Expr::Var(f)), self.exprs(args));
                }
                let recv = self.expr(recv);
                let m = if self.is_local(m) {
                    m.clone()
                } else {
                    let q = self.scope.get(m).cloned().unwrap_or_else(|| m.clone());
                    self.written(m, &q)
                };
                Expr::Method(Box::new(recv), m, self.exprs(args))
            }
            Expr::Field(r, f) => {
                if let Some(path) = self.module_path(r) {
                    let q = self.name(&format!("{path}.{f}"));
                    if self.writing() {
                        let (qp, last) = q.rsplit_once('.').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((path.clone(), f.clone()));
                        let r = if qp == path { r.clone() } else { Box::new(path_expr(&qp)) };
                        return Expr::Field(r, last);
                    }
                    return Expr::Var(q);
                }
                Expr::Field(Box::new(self.expr(r)), f.clone())
            }
            Expr::Mock(t) => Expr::Mock(self.ty(t)),
            Expr::Interp(parts) => Expr::Interp(
                parts
                    .iter()
                    .map(|p| match p {
                        StrPart::Lit(l) => StrPart::Lit(l.clone()),
                        StrPart::Expr(e, spec) => StrPart::Expr(self.expr(e), spec.clone()),
                    })
                    .collect(),
            ),
            Expr::Return(x) => Expr::Return(Box::new(self.expr(x))),
            Expr::Range(lo, hi, inclusive) => {
                let lo = lo.as_ref().map(|x| Box::new(self.expr(x)));
                Expr::Range(lo, hi.as_ref().map(|x| Box::new(self.expr(x))), *inclusive)
            }
            Expr::Slice(x, r) => Expr::Slice(Box::new(self.expr(x)), Box::new(self.expr(r))),
            Expr::Comp(elem, gens) => {
                // each `for` binds its pattern for the conditions and `for`s after it, and the element
                let mut out = Vec::new();
                for g in gens {
                    let iter = self.expr(&g.iter);
                    self.push(&[]);
                    let pat = self.pat(&g.pat);
                    let cond = g.cond.as_ref().map(|c| self.expr(c));
                    out.push(CompFor { pat, iter, cond });
                }
                let elem = self.expr(elem);
                gens.iter().for_each(|_| self.pop());
                Expr::Comp(Box::new(elem), out)
            }
            Expr::Mut(x) => Expr::Mut(Box::new(self.expr(x))),
            Expr::Try(x, k) => Expr::Try(Box::new(self.expr(x)), *k),
            Expr::Except(x, name, fb) => {
                let x = self.expr(x);
                self.push(&name.iter().cloned().collect::<Vec<_>>());
                let fb = self.expr(fb);
                self.pop();
                Expr::Except(Box::new(x), name.clone(), Box::new(fb))
            }
            Expr::Lambda(ps, b) => {
                let ps: Vec<LamParam> = ps.iter().map(|p| LamParam { name: p.name.clone(), ty: p.ty.as_ref().map(|t| self.ty(t)) }).collect();
                self.push(&ps.iter().map(|p| p.name.clone()).collect::<Vec<_>>());
                let b = self.expr(b);
                self.pop();
                Expr::Lambda(ps, Box::new(b))
            }
            Expr::Bin(op, l, r) => Expr::Bin(*op, Box::new(self.expr(l)), Box::new(self.expr(r))),
            Expr::Un(op, x) => Expr::Un(op.clone(), Box::new(self.expr(x))),
            Expr::Is(x, c) => Expr::Is(Box::new(self.expr(x)), self.name(c)),
            Expr::If(c, t, f) => Expr::If(Box::new(self.expr(c)), Box::new(self.expr(t)), Box::new(self.expr(f))),
            Expr::Match(s, name, arms) => {
                let s = self.expr(s);
                let arms = arms
                    .iter()
                    .map(|a| {
                        self.push(&name.iter().cloned().collect::<Vec<_>>());
                        let pat = self.pat(&a.pat);
                        let body = self.expr(&a.body);
                        self.pop();
                        Arm { comments: a.comments.clone(), pat, body }
                    })
                    .collect();
                Expr::Match(Box::new(s), name.clone(), arms)
            }
            Expr::Block(stmts, last) => {
                self.push(&[]);
                let stmts = stmts
                    .iter()
                    .map(|st| match st {
                        Stmt::Let(p, t, x) => {
                            let x = self.expr(x);
                            let t = t.as_ref().map(|t| self.ty(t));
                            Stmt::Let(self.pat(p), t, x)
                        }
                        Stmt::Var(n, t, x) => {
                            let x = self.expr(x);
                            let t = t.as_ref().map(|t| self.ty(t));
                            self.bind(n);
                            Stmt::Var(n.clone(), t, x)
                        }
                        Stmt::Assign(p, x) => Stmt::Assign(self.expr(p), self.expr(x)),
                        Stmt::While(c, b) => Stmt::While(self.expr(c), self.expr(b)),
                        Stmt::Loop(b) => Stmt::Loop(self.expr(b)),
                        Stmt::Break => Stmt::Break,
                        Stmt::Continue => Stmt::Continue,
                        Stmt::For(p, it, b) => {
                            let it = self.expr(it);
                            self.push(&[]);
                            let p = self.pat(p);
                            let b = self.expr(b);
                            self.pop();
                            Stmt::For(p, it, b)
                        }
                        Stmt::Expr(x) => Stmt::Expr(self.expr(x)),
                        Stmt::Assert(x) => Stmt::Assert(self.expr(x)),
                        Stmt::Comment(c) => Stmt::Comment(c.clone()),
                    })
                    .collect();
                let last = self.expr(last);
                self.pop();
                Expr::Block(stmts, Box::new(last))
            }
            _ => e.clone(),
        }
    }

    fn bounds(&mut self, bs: &[Bound]) -> Vec<Bound> {
        bs.iter().map(|b| Bound { param: b.param.clone(), traits: b.traits.iter().map(|t| self.ty(t)).collect() }).collect()
    }

    /// A function, an impl method or a trait method; `name` is its resolved name.
    fn func(&mut self, f: &FnDecl, name: String) -> FnDecl {
        self.tparams.push(f.type_params.iter().cloned().collect());
        let bounds = self.bounds(&f.bounds);
        let params: Vec<Param> = f.params.iter().map(|p| Param { name: p.name.clone(), ty: self.ty(&p.ty), mutable: p.mutable }).collect();
        let names: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
        self.push(&names);
        let ret = self.ty(&f.ret);
        let requires = self.exprs(&f.requires);
        let body = self.expr(&f.body);
        self.push(&["it".into()]);
        let ensures = self.exprs(&f.ensures);
        self.pop();
        self.pop();
        self.tparams.pop();
        FnDecl { doc: f.doc.clone(), comments: f.comments.clone(), name, type_params: f.type_params.clone(), bounds, params, ret, requires, ensures, terminates: f.terminates, body }
    }

    fn decl(&mut self, k: &DeclKind) -> DeclKind {
        let m = self.module.to_string();
        match k {
            DeclKind::Type(t) => {
                self.tparams.push(t.params.iter().cloned().collect());
                let body = match &t.body {
                    TypeBody::Alias(a) => TypeBody::Alias(self.ty(a)),
                    TypeBody::Record(fs) => TypeBody::Record(fs.iter().map(|f| Field { comments: f.comments.clone(), doc: f.doc.clone(), name: f.name.clone(), ty: self.ty(&f.ty) }).collect()),
                    TypeBody::Sum(vs) => TypeBody::Sum(
                        vs.iter()
                            .map(|v| Variant {
                                comments: v.comments.clone(),
                                name: self.declared(&m, &v.name),
                                payload: match &v.payload {
                                    Payload::None => Payload::None,
                                    Payload::Positional(ts) => Payload::Positional(ts.iter().map(|t| self.ty(t)).collect()),
                                    Payload::Record(fs) => Payload::Record(fs.iter().map(|f| Field { comments: f.comments.clone(), doc: f.doc.clone(), name: f.name.clone(), ty: self.ty(&f.ty) }).collect()),
                                },
                            })
                            .collect(),
                    ),
                };
                self.push(&["it".into()]);
                let refinement = t.refinement.as_ref().map(|r| self.expr(r));
                self.pop();
                self.tparams.pop();
                DeclKind::Type(TypeDecl { distinct: t.distinct, name: self.declared(&m, &t.name), params: t.params.clone(), body, refinement })
            }
            DeclKind::Fn(f) => DeclKind::Fn(self.func(f, self.declared(&m, &f.name))),
            DeclKind::Test(t) => {
                let name = if m.is_empty() || self.writing() { t.name.clone() } else { format!("{m}: {}", t.name) };
                DeclKind::Test(TestDecl { name, body: self.expr(&t.body) })
            }
            DeclKind::Trait(t) => {
                self.tparams.push(t.params.iter().cloned().collect());
                let supers = t.supers.iter().map(|s| self.ty(s)).collect();
                let methods = t
                    .methods
                    .iter()
                    .map(|tm| TraitMethod { comments: tm.comments.clone(), doc: tm.doc.clone(), decl: self.func(&tm.decl, self.declared(&m, &tm.decl.name)), default: tm.default })
                    .collect();
                let laws = t
                    .laws
                    .iter()
                    .map(|l| {
                        let params: Vec<Param> = l.params.iter().map(|p| Param { name: p.name.clone(), ty: self.ty(&p.ty), mutable: p.mutable }).collect();
                        let names: Vec<String> = l.params.iter().map(|p| p.name.clone()).collect();
                        self.push(&names);
                        let body = self.expr(&l.body);
                        self.pop();
                        Law { comments: l.comments.clone(), doc: l.doc.clone(), name: l.name.clone(), params, body }
                    })
                    .collect();
                self.tparams.pop();
                DeclKind::Trait(TraitDecl { name: self.declared(&m, &t.name), params: t.params.clone(), supers, methods, laws })
            }
            DeclKind::Impl(i) => {
                self.tparams.push(i.type_params.iter().cloned().collect());
                let bounds = self.bounds(&i.bounds);
                let trait_q = if i.is_inherent() { String::new() } else { self.lookup(&i.trait_name) };
                let trait_name = if i.is_inherent() { String::new() } else { self.written(&i.trait_name, &trait_q) };
                let trait_args = i.trait_args.iter().map(|a| self.ty(a)).collect();
                let for_type = self.ty(&i.for_type);
                if i.is_inherent() {
                    let head = crate::types::texpr_head(&for_type);
                    let short = short(&head).to_string();
                    if head.contains('.') && module_of(&head) != m {
                        self.errors.push(Diag { decl: self.decl.clone(), msg: format!("`impl {short}` must be in the module that defines `{short}` (`{}`)", module_of(&head)) });
                    }
                }
                // impl methods are named like the trait's methods (in the trait's module,
                // whatever the trait is called here); a type's own, like the type
                let tm = if i.is_inherent() { m.clone() } else { module_of(&trait_q).to_string() };
                let methods = i.methods.iter().map(|f| self.func(f, self.declared(&tm, &f.name))).collect();
                self.tparams.pop();
                DeclKind::Impl(ImplDecl { type_params: i.type_params.clone(), bounds, trait_name, trait_args, for_type, methods })
            }
            DeclKind::Use(u) => DeclKind::Use(u.clone()),
        }
    }
}

/// The names a move changes: the definition, and its variants or methods
/// (named in their module): old qualified name → new qualified name.
fn moved_names(p: &Program, old: &str, target: &str) -> Result<HashMap<String, String>, String> {
    let d = p.decls.iter().find(|d| d.qname() == old).ok_or(format!("no definition `{old}`"))?;
    let mut out = HashMap::new();
    out.insert(old.to_string(), qualify(target, &d.name()));
    match &d.kind {
        DeclKind::Fn(_) => {}
        DeclKind::Type(t) => {
            if let TypeBody::Sum(vs) = &t.body {
                for v in vs {
                    out.insert(qualify(&d.module, &v.name), qualify(target, &v.name));
                }
            }
        }
        DeclKind::Trait(t) => {
            for m in &t.methods {
                out.insert(qualify(&d.module, &m.decl.name), qualify(target, &m.decl.name));
            }
        }
        _ => return Err(format!("only functions, types and traits can be moved (`{old}` is a {})", d.kind_str())),
    }
    Ok(out)
}

/// Make module `m` import `item` from `path` (unless it already can).
fn add_import(p: &mut Program, m: &str, path: &str, item: &str) {
    if m == path || path.is_empty() {
        return;
    }
    for d in p.decls.iter_mut().filter(|d| d.module == m) {
        if let DeclKind::Use(u) = &mut d.kind {
            if u.path == path {
                match &mut u.items {
                    None => return,
                    Some(items) => {
                        if !items.iter().any(|i| i == item) {
                            items.push(item.to_string());
                        }
                        return;
                    }
                }
            }
        }
    }
    // a new `use`, after the module's last one (or before its first definition)
    let at = p
        .decls
        .iter()
        .rposition(|d| d.module == m && matches!(d.kind, DeclKind::Use(_)))
        .map(|i| i + 1)
        .or_else(|| p.decls.iter().position(|d| d.module == m))
        .unwrap_or(p.decls.len());
    p.decls.insert(at, Decl { comments: vec![], doc: vec![], kind: DeclKind::Use(UseDecl { path: path.to_string(), items: Some(vec![item.to_string()]) }), module: m.to_string(), public: false });
}

/// Moving `old` into module `target` (no checks): the definition changes
/// module, references written with its module follow it, short ones stay
/// short and their module imports it, and `use` items that named it go.
/// Returns the program and the definition's new qualified name.
fn move_apply(p: &Program, old: &str, target: &str) -> Result<(Program, String), String> {
    let moves = moved_names(p, old, target)?;
    let new_q = moves[old].clone();
    let i = p.decls.iter().position(|d| d.qname() == old).unwrap();
    let (out, needs) = run_with(p, None, Some(&moves));
    let mut program = out.program;
    for (r, d) in program.decls.iter_mut().zip(&p.decls) {
        r.module = d.module.clone();
    }
    program.decls[i].module = target.to_string();
    // what the moved definition uses from its old module (or that module's imports)
    // must be imported where it goes: names it writes short
    let written = names_in(&p.decls[i].kind);
    let deps = crate::store::graph(p).get(old).cloned().unwrap_or_default();
    let mut imports: Vec<(String, String, String)> = Vec::new();
    for dep in deps {
        let m = module_of(&dep).to_string();
        let short = short(&dep).to_string();
        if m != target && !m.is_empty() && !moves.contains_key(&dep) && written.contains(&short) {
            imports.push((target.to_string(), m, short));
        }
    }
    // modules that write a moved name short import it from its new module
    for (k, ns) in needs.iter().enumerate() {
        if k == i {
            continue; // the moved definition's own references (recursion) are local where it goes
        }
        for nq in ns {
            let m = p.decls[k].module.clone();
            // a variant or method is imported by its own name
            imports.push((m, target.to_string(), short(&nq).to_string()));
        }
    }
    // emptied `use` lists go
    let emptied: Vec<usize> = program
        .decls
        .iter()
        .zip(&p.decls)
        .enumerate()
        .filter(|(_, (a, b))| matches!((&a.kind, &b.kind), (DeclKind::Use(x), DeclKind::Use(y)) if x.items.as_ref().is_some_and(|v| v.is_empty()) && y.items.as_ref().is_some_and(|v| !v.is_empty())))
        .map(|(k, _)| k)
        .collect();
    for k in emptied.into_iter().rev() {
        program.decls.remove(k);
    }
    imports.sort();
    imports.dedup();
    for (m, path, item) in imports {
        add_import(&mut program, &m, &path, &item);
    }
    Ok((program, new_q))
}

/// Move the function, type or trait `old` (a qualified name) to module
/// `target`, keeping its short name, and update everything that refers to
/// it. Refused when something private would end up out of reach, or when
/// any name would refer to something else afterwards.
pub fn move_def(p: &Program, old: &str, target: &str) -> Result<Program, String> {
    let d = p.decls.iter().find(|d| d.qname() == old).ok_or(format!("no definition `{old}`"))?;
    let source = d.module.clone();
    if source == target {
        return Err(format!("`{old}` is already in module `{}`", if target.is_empty() { "main" } else { target }));
    }
    let moves = moved_names(p, old, target)?;
    let taken = defined(p);
    if let Some((_, n)) = moves.iter().find(|(_, n)| taken.contains(*n)) {
        return Err(format!("`{n}` already exists"));
    }
    // private things must stay in reach
    let graph = crate::store::graph(p);
    let public: HashMap<String, bool> = p.decls.iter().map(|d| (d.qname(), d.public)).collect();
    for dep in graph.get(old).into_iter().flatten() {
        let m = module_of(dep);
        if !m.is_empty() && m != target && !public.get(dep).copied().unwrap_or(true) {
            return Err(format!("`{old}` uses `{dep}`, which is private to `{m}`: make it `pub`, or move it too"));
        }
    }
    if !d.public {
        let users: Vec<&String> = graph.iter().filter(|(n, ds)| ds.contains(old) && module_of(n) != target && *n != old).map(|(n, _)| n).collect();
        if !users.is_empty() {
            return Err(format!(
                "`{old}` is private and {} use{} it from outside `{}`: make it `pub` first",
                users.iter().map(|u| format!("`{u}`")).collect::<Vec<_>>().join(", "),
                if users.len() == 1 { "s" } else { "" },
                if target.is_empty() { "main" } else { target }
            ));
        }
    }
    let (program, _) = move_apply(p, old, target)?;
    // every name must refer to what it did, with the moved names moved
    let expect: Vec<Decl> = resolved_defs(p).iter().map(|d| renamed(d, &moves)).collect();
    if resolved_defs(&program) != expect {
        return Err(format!("moving `{old}` to `{target}` would change what some names refer to (something visible there already has its name)"));
    }
    Ok(program)
}

/// After a merge: `merged` has the definition `old` moved (to `new`, in
/// another module) but also references to it where it was. Put it back,
/// replay the move, and check, definition by definition, that what the move
/// touched resolves as before (with the moved names moved) and everything
/// else exactly as merged. `None` when there is nothing to follow.
pub fn follow_move(merged: &Program, old: &str, new: &str) -> Option<Program> {
    if merged.decls.iter().any(|d| d.qname() == old) {
        return None; // defined where it was again
    }
    let i = merged.decls.iter().position(|d| d.qname() == new)?;
    let (source, target) = (module_of(old).to_string(), module_of(new).to_string());
    let mut back = merged.clone();
    back.decls[i].module = source;
    let moves = moved_names(&back, old, &target).ok()?;
    let (program, _) = move_apply(&back, old, &target).ok()?;
    let by_name = |p: &Program| -> HashMap<String, Decl> { resolve(p).program.decls.into_iter().zip(&p.decls).filter(|(_, d)| !matches!(d.kind, DeclKind::Use(_))).map(|(r, d)| (d.qname(), r)).collect() };
    let (rm, rb, ra) = (by_name(merged), by_name(&back), by_name(&program));
    let tb: HashMap<String, Decl> = back.decls.iter().filter(|d| !matches!(d.kind, DeclKind::Use(_))).map(|d| (d.qname(), d.clone())).collect();
    for d in program.decls.iter().filter(|d| !matches!(d.kind, DeclKind::Use(_))) {
        let q = d.qname();
        let touched = q != new && tb.get(&q).is_some_and(|b| b != d);
        let expect = if touched {
            renamed(rb.get(&q)?, &moves)
        } else {
            rm.get(&q)?.clone()
        };
        if ra.get(&q)? != &expect {
            return None;
        }
    }
    // the definitions, or only a use list (an import the other side's code needs)
    let changed = program.decls.len() != merged.decls.len() || program.decls.iter().zip(&merged.decls).any(|(a, b)| a != b);
    changed.then_some(program)
}
