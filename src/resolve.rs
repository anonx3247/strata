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

use crate::names::{module_of, qualified, qualify, short};
use crate::ast::*;
use crate::types::Diag;
use crate::fx::{HashMap, HashSet};

#[derive(Clone, Debug)]
struct Item {
    module: Symbol,
    public: bool,
    /// An `extern` function: callable only from its own module.
    foreign: bool,
    /// A mock (`mock MemFiles = { ... }`): for tests only.
    mock: bool,
}

pub struct Resolution {
    pub program: Program,
    pub errors: Vec<Diag>,
}

/// Each `capability Sqlite { fn open(...) }` as the checker, the compiler
/// and `verify` see it: the capability itself, its functions taken out, and
/// an implementation of the type's own methods (`impl Sqlite { fn open(self,
/// ...) }`), each called on the capability (which holds nothing). The
/// implementations go after every other definition, so indices are kept.
fn capabilities(p: &Program) -> std::borrow::Cow<'_, Program> {
    if !p.decls.iter().any(|d| matches!(&d.kind, DeclKind::Capability(c) if !c.methods.is_empty())) {
        return std::borrow::Cow::Borrowed(p);
    }
    let mut out = p.clone();
    let mut impls = vec![];
    for d in &mut out.decls {
        let DeclKind::Capability(c) = &mut d.kind else { continue };
        let methods: Vec<FnDecl> = std::mem::take(&mut c.methods)
            .into_iter()
            .map(|mut m| {
                m.params.insert(0, Param { name: Symbol::new("self"), ty: TypeExpr::Named(Symbol::new("Self"), vec![]), mutable: false, comptime: false });
                m
            })
            .collect();
        let imp = ImplDecl { type_params: vec![], bounds: vec![], trait_name: Symbol::EMPTY, trait_args: vec![], for_type: TypeExpr::Named(c.name, vec![]), methods, derived: false };
        impls.push(Decl { comments: vec![], doc: vec![], kind: DeclKind::Impl(imp), module: d.module, public: d.public });
    }
    out.decls.extend(impls);
    std::borrow::Cow::Owned(out)
}

pub fn resolve(p: &Program) -> Resolution {
    let p = &*capabilities(p);
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
    mock_methods(&mut r.program);
    static_calls(&mut r.program);
    crate::desugar::program(&mut r.program, &mut r.errors);
    crate::derive::expand(&mut r.program, &mut r.errors);
    r
}

/// The methods an implementation for a mock leaves out (`impl Files for
/// MemFiles` with only `read` and `write`), filled in: each with the trait's
/// signature, answered by a generated mock of the trait (`{ mut m := mock
/// Files; mut m.exists(path) }`), which checks the method's `requires` and
/// gives a result within its type and `ensures`. Methods without `self` and
/// default methods are left as they are.
fn mock_methods(p: &mut Program) {
    let mocks: HashSet<String> = p.decls.iter().filter_map(|d| match &d.kind {
        DeclKind::Type(t) if t.mock => Some(qualify(&d.module, &t.name)),
        _ => None,
    }).collect();
    if mocks.is_empty() {
        return;
    }
    let traits: HashMap<String, TraitDecl> = p.decls.iter().filter_map(|d| match &d.kind {
        DeclKind::Trait(t) => Some((qualify(&d.module, &t.name), t.clone())),
        _ => None,
    }).collect();
    for d in &mut p.decls {
        let DeclKind::Impl(i) = &mut d.kind else { continue };
        if i.is_inherent() || !mocks.contains(&qualify(&d.module, &crate::types::texpr_head(&i.for_type))) && !mocks.contains(crate::types::texpr_head(&i.for_type).as_str()) {
            continue;
        }
        let Some(tr) = traits.get(i.trait_name.as_str()).or_else(|| traits.get(&qualify(&d.module, &i.trait_name))) else { continue };
        let sub: HashMap<Symbol, TypeExpr> = tr.params.iter().copied().zip(i.trait_args.iter().cloned()).collect();
        for tm in &tr.methods {
            if tm.default || !takes_self(&tm.decl) || i.methods.iter().any(|m| short(&m.name) == short(&tm.decl.name)) {
                continue;
            }
            let mut f = tm.decl.clone();
            f.doc = vec![];
            f.comments = vec![];
            f.requires = vec![];
            f.ensures = vec![];
            for p in &mut f.params {
                p.ty = crate::types::subst_texpr(&p.ty, &sub);
            }
            f.ret = crate::types::subst_texpr(&f.ret, &sub);
            let m = Symbol::new("generated");
            let mutating = f.params[0].mutable;
            let args: Vec<Expr> = f.params[1..].iter().map(|p| if p.mutable { Expr::Mut(Box::new(Expr::Var(p.name))) } else { Expr::Var(p.name) }).collect();
            // added after resolution: the method by its resolved (qualified) name
            let call = Expr::Method(Box::new(Expr::Var(m)), tm.decl.name, args);
            let made = Expr::Mock(TypeExpr::Named(i.trait_name, i.trait_args.clone()));
            f.body = Expr::Block(
                vec![if mutating { Stmt::Var(m, None, made) } else { Stmt::Let(Pattern::Bind(m), None, made) }],
                Box::new(if mutating { Expr::Mut(Box::new(call)) } else { call }),
            );
            i.methods.push(f);
        }
    }
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
    let mut statics: HashSet<(String, String)> = HashSet::default();
    for d in &p.decls {
        if let DeclKind::Impl(i) = &d.kind {
            if i.is_inherent() {
                for m in i.methods.iter().filter(|m| !takes_self(m)) {
                    statics.insert(((crate::types::texpr_head(&i.for_type)).to_string(), short(&m.name)));
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
                    if statics.contains(&(t.to_string(), short(m))) {
                        *e = Expr::Call(Box::new(Expr::Var(static_name(t, m).into())), std::mem::take(args));
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
                DeclKind::Fn(_) => Some(("function", d.module.to_string())),
                DeclKind::Type(_) => Some(("type", d.module.to_string())),
                DeclKind::Trait(_) => Some(("trait", d.module.to_string())),
                DeclKind::Extern(ExternDecl { item: ExternItem::Fn(_), .. }) => Some(("function", d.module.to_string())),
                DeclKind::Extern(ExternDecl { item: ExternItem::Type(_), .. }) | DeclKind::Capability(_) => Some(("type", d.module.to_string())),
                _ => None,
            };
        }
        match &d.kind {
            DeclKind::Type(t) => {
                if let TypeBody::Sum(vs) = &t.body {
                    if vs.iter().any(|v| qualify(&d.module, &v.name) == q) {
                        return Some(("variant", d.module.to_string()));
                    }
                }
            }
            DeclKind::Trait(t) => {
                if t.methods.iter().any(|m| qualify(&d.module, &m.decl.name) == q) {
                    return Some(("method", d.module.to_string()));
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
                DeclKind::Fn(f) => f.name = Symbol::from(new),
                DeclKind::Type(t) => t.name = Symbol::from(new),
                DeclKind::Trait(t) => t.name = Symbol::from(new),
                DeclKind::Extern(e) => match &mut e.item {
                    ExternItem::Fn(f) => f.name = Symbol::from(new),
                    ExternItem::Type(t) => *t = Symbol::from(new),
                },
                DeclKind::Capability(c) => c.name = Symbol::from(new),
                _ => {}
            }
            continue;
        }
        match &mut d.kind {
            DeclKind::Type(t) => {
                let tq = qualify(&m, &t.name);
                match &mut t.body {
                    TypeBody::Record(fs) if kind == "field" && tq == of => fs.iter_mut().filter(|f| f.name == short).for_each(|f| f.name = Symbol::from(new)),
                    TypeBody::Sum(vs) => {
                        for v in vs.iter_mut() {
                            let vq = qualify(&m, &v.name);
                            if kind == "variant" && vq == q {
                                v.name = Symbol::from(new);
                            }
                            if let (Payload::Record(fs), true) = (&mut v.payload, kind == "field" && vq == of) {
                                fs.iter_mut().filter(|f| f.name == short).for_each(|f| f.name = Symbol::from(new));
                            }
                        }
                    }
                    _ => {}
                }
            }
            DeclKind::Trait(t) if kind == "method" => {
                for tm in t.methods.iter_mut().filter(|tm| qualify(&m, &tm.decl.name) == q) {
                    tm.decl.name = Symbol::from(new);
                }
            }
            // an impl of the method's trait (method names are unique across traits)
            DeclKind::Impl(i) if kind == "method" => {
                for f in i.methods.iter_mut().filter(|f| f.name == short) {
                    f.name = Symbol::from(new);
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
            TypeBody::Record(fs) if qualify(&d.module, &t.name) == owner => return fs.iter().map(|f| f.name.to_string()).collect(),
            TypeBody::Sum(vs) => {
                for v in vs {
                    if let Payload::Record(fs) = &v.payload {
                        if qualify(&d.module, &v.name) == owner {
                            return fs.iter().map(|f| f.name.to_string()).collect();
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
                TypeBody::Record(fs) => vec![(qualify(&d.module, &t.name), fs.iter().map(|f| f.name.to_string()).collect())],
                TypeBody::Sum(vs) => vs
                    .iter()
                    .filter_map(|v| match &v.payload {
                        Payload::Record(fs) => Some((qualify(&d.module, &v.name), fs.iter().map(|f| f.name.to_string()).collect())),
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
    let mut modules: HashSet<String> = HashSet::default();
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
                    TypeBody::Record(fs) if tq == *owner => fs.iter_mut().filter(|x| x.name == *f).for_each(|x| x.name = Symbol::from(new.as_str())),
                    TypeBody::Sum(vs) => {
                        for v in vs.iter_mut().filter(|v| qualify(&d.module, &v.name) == *owner) {
                            if let Payload::Record(fs) = &mut v.payload {
                                fs.iter_mut().filter(|x| x.name == *f).for_each(|x| x.name = Symbol::from(new.as_str()));
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
        fn rename<T>(&self, c: &str, fs: &mut [(Symbol, T)]) {
            if let Some(new) = self.ctor_new.filter(|_| short(c) == self.ctor) {
                fs.iter_mut().filter(|(n, _)| *n == self.field).for_each(|(n, _)| *n = new.into());
            }
        }
    }
    impl VisitMut for Uses<'_> {
        fn expr(&mut self, e: &mut Expr) {
            match e {
                // `x.qty`, but not `orders.qty` (a qualified name)
                Expr::Field(r, f) if f == self.field && !place_path(r).is_some_and(|m| self.modules.contains(&m)) => {
                    if let Some(n) = (self.access)(self.field) {
                        *f = n.into();
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
    let mut out = HashSet::default();
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
    if p.decls.iter().any(|d| d.qname() == old && matches!(&d.kind, DeclKind::Extern(ExternDecl { item: ExternItem::Fn(_), .. }))) {
        return Err(format!("`{old}` is a foreign function: its name is the symbol it is linked to, so it can't be renamed (wrap it in a function with the name you want)"));
    }
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
    let map = HashMap::from_iter([(old.to_string(), new_q.clone())]);
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
    let map = HashMap::from_iter([(old.to_string(), new_q.clone())]);
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
fn names_mut(d: &mut DeclKind, f: &mut dyn FnMut(&mut Symbol)) {
    struct Names<'a>(&'a mut dyn FnMut(&mut Symbol));
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
        DeclKind::Capability(c) => {
            f(&mut c.name);
            c.methods.iter_mut().for_each(|m| f(&mut m.name));
        }
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
        DeclKind::Extern(e) => match &mut e.item {
            ExternItem::Fn(fd) => f(&mut fd.name),
            ExternItem::Type(t) => f(t),
        },
    }
}

/// The names in a definition, as `names_mut` visits them.
fn names_in(d: &DeclKind) -> HashSet<Symbol> {
    let mut out = HashSet::default();
    names_mut(&mut d.clone(), &mut |n| {
        out.insert(*n);
    });
    out
}

/// A definition with names replaced: `renames` maps old qualified names to new.
fn renamed(d: &Decl, renames: &HashMap<String, String>) -> Decl {
    let mut d = d.clone();
    names_mut(&mut d.kind, &mut |n| {
        if let Some(new) = renames.get(n.as_str()) {
            *n = new.into();
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
    run_with(p, rename, None, None).0
}

/// How code is stored: every reference to a definition by its qualified
/// name (`orders.Line`, whatever `use` brought it in as), the definitions'
/// own names short, and nothing else rewritten. Names that resolve to
/// nothing stay as written; locals and method names are left alone.
pub fn stored_form(p: &Program) -> Program {
    store(p, Store::Qualify)
}

/// Stored code as it is written: each qualified reference as short as its
/// module's scope allows (`Line` where `use orders.{Line}` imports it,
/// `orders.Line` otherwise, or where a local hides `Line`), and a reference
/// to another module's definition as the parser reads `orders.f(x)`.
pub fn written_form(p: &Program) -> Program {
    store(p, Store::Shorten)
}

fn store(p: &Program, mode: Store) -> Program {
    // the standard library's names are in scope for those who `use` it
    let full = crate::stdlib::with_std(p);
    let (r, _) = run_with(full.as_ref().unwrap_or(p), None, None, Some(mode));
    let mut decls = r.program.decls;
    decls.truncate(p.decls.len());
    for (d, o) in decls.iter_mut().zip(&p.decls) {
        d.module = o.module;
    }
    Program { decls }
}

/// A root-module function's name while the program is resolved to be
/// checked or run: one named like a builtin (`get`) is `main$get`, so the
/// other modules' calls of the builtin (the standard library's too) still
/// reach the builtin. The root module's own uses of `get` mean its function.
fn root_fn(module: Symbol, name: Symbol, resolving: bool) -> Symbol {
    if resolving && module.is_empty() && crate::types::BUILTINS.iter().any(|b| b.name == name.as_str()) {
        Symbol::new(&format!("{ROOT_FN}{name}"))
    } else {
        qualified(module, name)
    }
}

/// The prefix of a root-module function named like a builtin (`main$get`).
pub const ROOT_FN: &str = "main$";

#[derive(Clone, Copy, PartialEq)]
enum Store {
    Qualify,
    Shorten,
}

/// Resolve, or write back (with a rename, or with `moves`: old qualified
/// name → new qualified name). Also gives, per definition, the moved names
/// it refers to by their short name from another module than theirs, which
/// that module must then import.
fn run_with(p: &Program, rename: Option<(String, String)>, moves: Option<&HashMap<String, String>>, store: Option<Store>) -> (Resolution, Vec<Vec<String>>) {
    let rename: Option<(Symbol, Symbol)> = rename.map(|(o, n)| (o.into(), n.into()));
    let moves: Option<HashMap<Symbol, Symbol>> = moves.map(|m| m.iter().map(|(o, n)| (o.sym(), n.sym())).collect());
    let resolving = store.is_none() && rename.is_none() && moves.is_none();
    let mut table: HashMap<Symbol, Item> = HashMap::default();
    // children of a type (its constructors) or a trait (its methods), as (short, qualified)
    let mut children: HashMap<Symbol, Vec<(Symbol, Symbol)>> = HashMap::default();
    let mut modules: HashSet<Symbol> = HashSet::default();
    // qualified names of trait methods
    let mut methods: HashSet<Symbol> = HashSet::default();
    let add_modules = |modules: &mut HashSet<Symbol>, mut m: &str| {
        while !m.is_empty() {
            modules.insert(m.sym());
            m = module_of(m);
        }
    };
    for d in &p.decls {
        add_modules(&mut modules, &d.module);
        let item = Item { module: d.module, public: d.public, foreign: matches!(&d.kind, DeclKind::Extern(ExternDecl { item: ExternItem::Fn(_), .. })), mock: matches!(&d.kind, DeclKind::Type(t) if t.mock) };
        match &d.kind {
            DeclKind::Capability(c) => {
                table.insert(qualified(d.module, c.name), item.clone());
            }
            DeclKind::Type(t) => {
                let q = qualified(d.module, t.name);
                table.insert(q, item.clone());
                if let TypeBody::Sum(vs) = &t.body {
                    for v in vs {
                        let cq = qualified(d.module, v.name);
                        table.insert(cq, item.clone());
                        children.entry(q).or_default().push((v.name, cq));
                    }
                }
            }
            DeclKind::Trait(t) => {
                let q = qualified(d.module, t.name);
                table.insert(q, item.clone());
                for m in &t.methods {
                    let mq = qualified(d.module, m.decl.name);
                    table.insert(mq, item.clone());
                    methods.insert(mq);
                    children.entry(q).or_default().push((m.decl.name, mq));
                }
            }
            DeclKind::Fn(f) | DeclKind::Extern(ExternDecl { item: ExternItem::Fn(f), .. }) => {
                table.insert(root_fn(d.module, f.name, resolving), item);
            }
            DeclKind::Extern(ExternDecl { item: ExternItem::Type(t), .. }) => {
                table.insert(qualified(d.module, *t), item);
            }
            // a type's own methods come with the type, like a trait's
            DeclKind::Impl(i) if i.is_inherent() => {
                // (stored code names the type in full)
                let head = crate::types::texpr_head(&i.for_type);
                let tq = if head.contains('.') { Symbol::new(&head) } else { qualified(d.module, Symbol::new(&head)) };
                for m in &i.methods {
                    let mq = qualified(d.module, m.name);
                    table.insert(mq, Item { module: d.module, public: true, foreign: false, mock: false });
                    children.entry(tq).or_default().push((m.name, mq));
                }
            }
            DeclKind::Test(_) | DeclKind::Impl(_) | DeclKind::Use(_) => {}
        }
    }

    // already-qualified names (a resolved program) imply their modules too
    for q in table.keys().copied().collect::<Vec<_>>() {
        add_modules(&mut modules, module_of(&q));
    }
    let mut errors = Vec::new();
    // what each module can refer to by short name
    let mut scopes: HashMap<Symbol, HashMap<Symbol, Symbol>> = HashMap::default();
    let mut all_modules: Vec<Symbol> = p.decls.iter().map(|d| d.module).collect();
    all_modules.sort();
    all_modules.dedup();
    for &m in &all_modules {
        let mut scope: HashMap<Symbol, Symbol> = HashMap::default();
        // the root module is visible everywhere (but a function of its named like
        // a builtin, `main$get`, only in it: elsewhere `get` is the builtin)
        for (&q, it) in &table {
            if it.module.is_empty() && !q.as_str().starts_with(ROOT_FN) {
                scope.insert(q, q);
            }
        }
        for d in p.decls.iter().filter(|d| d.module == m) {
            let DeclKind::Use(u) = &d.kind else { continue };
            if !modules.contains(&u.path) {
                errors.push(Diag { decl: d.qname(), msg: format!("there is no module `{}`", u.path) });
                continue;
            }
            match &u.items {
                None => {
                    for (&q, it) in &table {
                        if it.module == u.path && it.public {
                            scope.insert(Symbol::new(&q[u.path.len() + 1..]), q);
                        }
                    }
                }
                Some(items) => {
                    for &i in items {
                        let q = qualified(u.path, i);
                        match table.get(&q) {
                            None => errors.push(Diag { decl: d.qname(), msg: format!("module `{}` has no `{i}`", u.path) }),
                            Some(it) if !it.public => errors.push(Diag {
                                decl: d.qname(),
                                msg: format!("`{i}` is private to module `{}` (only `pub` definitions are part of its contract)", u.path),
                            }),
                            Some(_) => {
                                scope.insert(i, q);
                                // importing a type brings its constructors; a trait, its methods
                                for &(short, cq) in children.get(&q).into_iter().flatten() {
                                    scope.insert(short, cq);
                                }
                            }
                        }
                    }
                }
            }
        }
        // the module's own definitions win
        for (&q, it) in &table {
            if it.module == m {
                let short = q.as_str().strip_prefix(ROOT_FN).unwrap_or(&q[if m.is_empty() { 0 } else { m.len() + 1 }..]);
                scope.insert(Symbol::new(short), q);
            }
        }
        scopes.insert(m, scope);
    }

    let imports: HashMap<Symbol, Vec<Symbol>> = all_modules
        .iter()
        .map(|&m| (m, p.decls.iter().filter(|d| d.module == m).filter_map(|d| if let DeclKind::Use(u) = &d.kind { Some(u.path) } else { None }).collect()))
        .collect();
    // each module's `use`s: (module, the names it lists, or `None` for `*`)
    let uses: HashMap<Symbol, Vec<(Symbol, Option<Vec<Symbol>>)>> = all_modules
        .iter()
        .map(|&m| (m, p.decls.iter().filter(|d| d.module == m).filter_map(|d| if let DeclKind::Use(u) = &d.kind { Some((u.path, u.items.clone())) } else { None }).collect()))
        .collect();
    let mut decls = Vec::new();
    let mut needs = Vec::new();
    for d in &p.decls {
        let mut r = Rw {
            scope: &scopes[&d.module],
            table: &table,
            methods: &methods,
            modules: &modules,
            module: d.module,
            imports: &imports[&d.module],
            uses: &uses[&d.module],
            decl: d.qname(),
            for_tests: match &d.kind {
                DeclKind::Test(_) => true,
                DeclKind::Type(t) => t.mock,
                DeclKind::Impl(i) => {
                    let head = crate::types::texpr_head(&i.for_type).sym();
                    let q = scopes[&d.module].get(&head).copied().unwrap_or(if head.contains('.') { head } else { qualified(d.module, head) });
                    table.get(&q).is_some_and(|it| it.mock)
                }
                _ => false,
            },
            locals: vec![],
            tparams: vec![],
            errors: vec![],
            rename: rename.as_ref(),
            moves: moves.as_ref(),
            store,
            needs: Default::default(),
        };
        let kind = match (&d.kind, rename.as_ref()) {
            // `use orders.{ship}` no longer names a moved definition (the module imports it anew)
            (DeclKind::Use(u), _) if moves.is_some() => DeclKind::Use(UseDecl {
                path: u.path,
                items: u.items.as_ref().map(|is| is.iter().filter(|&&i| !moves.as_ref().unwrap().contains_key(&qualified(u.path, i))).copied().collect()),
            }),
            // `use core.{same_ids}` names what it imports
            (DeclKind::Use(u), Some(&(old, new))) => DeclKind::Use(UseDecl { path: u.path, items: u.items.as_ref().map(|is| is.iter().map(|&i| if qualified(u.path, i) == old { new } else { i }).collect()) }),
            _ => r.decl(&d.kind),
        };
        errors.extend(r.errors);
        needs.push(r.needs.into_inner().into_iter().map(String::from).collect());
        decls.push(Decl { comments: d.comments.clone(), doc: d.doc.clone(), kind, module: Symbol::EMPTY, public: d.public });
    }
    (Resolution { program: Program { decls }, errors }, needs)
}

/// `shop.orders` as an expression (a variable and field accesses).
fn path_expr(m: &str) -> Expr {
    let mut parts = m.split('.');
    let mut e = Expr::Var(parts.next().unwrap_or("").into());
    for p in parts {
        e = Expr::Field(Box::new(e), Symbol::from(p));
    }
    e
}

/// Builtins that a plain call reaches even when an imported trait has a
/// method of the same name.
const PRELUDE_FNS: &[&str] = &["print"];

struct Rw<'a> {
    scope: &'a HashMap<Symbol, Symbol>,
    table: &'a HashMap<Symbol, Item>,
    /// qualified names of trait methods
    methods: &'a HashSet<Symbol>,
    modules: &'a HashSet<Symbol>,
    module: Symbol,
    /// modules this one `use`s, for "that's private" hints
    imports: &'a [Symbol],
    /// the module's `use`s, for showing names of modules that aren't loaded
    uses: &'a [(Symbol, Option<Vec<Symbol>>)],
    decl: String,
    /// The definition may use mocks: a test, a mock, or an implementation for one.
    for_tests: bool,
    locals: Vec<HashSet<Symbol>>,
    tparams: Vec<HashSet<Symbol>>,
    errors: Vec<Diag>,
    /// Rename mode: (old qualified name, new short name). Names are then
    /// written back as they were, except those that resolve to `old`.
    rename: Option<&'a (Symbol, Symbol)>,
    /// Move mode: old qualified name → new qualified name. Names are written
    /// back as they were; a qualified one that refers to a moved definition
    /// gets its new module, and a short one stays short (see `needs`).
    moves: Option<&'a HashMap<Symbol, Symbol>>,
    /// moved names this definition writes short, from a module that must import them
    needs: std::cell::RefCell<Vec<Symbol>>,
    /// Converting between the written and the stored form (see `stored_form`)
    store: Option<Store>,
}

impl Rw<'_> {
    fn is_local(&self, x: Symbol) -> bool {
        self.locals.iter().any(|s| s.contains(&x))
    }
    fn bind(&mut self, x: Symbol) {
        if let Some(s) = self.locals.last_mut() {
            s.insert(x);
        }
    }
    fn push(&mut self, names: &[Symbol]) {
        self.locals.push(names.iter().copied().collect());
    }
    fn pop(&mut self) {
        self.locals.pop();
    }

    /// Resolve a (possibly qualified) global name; in rename mode, give it
    /// back as written, renamed if it refers to the renamed definition.
    fn name(&mut self, n: Symbol) -> Symbol {
        let q = self.lookup(n);
        self.written(n, q)
    }
    /// Writing names back (rename or move), rather than resolving them.
    fn writing(&self) -> bool {
        self.rename.is_some() || self.moves.is_some()
    }
    fn written(&self, n: Symbol, q: Symbol) -> Symbol {
        match self.store {
            // only what names a definition is qualified
            // what this module's scope gives that name (a private name used
            // from outside stays as written, so its error reads the same)
            Some(Store::Qualify) => return if n == q || self.scope.get(&n) == Some(&q) { q } else { n },
            Some(Store::Shorten) => return self.shortest(n, q),
            None => {}
        }
        if let Some(&nq) = self.moves.and_then(|m| m.get(&q)) {
            if n.contains('.') {
                return nq;
            }
            if module_of(&nq) != self.module {
                self.needs.borrow_mut().push(nq);
            }
            return n;
        }
        match self.rename {
            None if self.moves.is_none() => q,
            None => n,
            Some(&(old, new)) if q == old => match n.rsplit_once('.') {
                Some((path, _)) => format!("{path}.{new}").into(),
                None => new,
            },
            Some(_) => n,
        }
    }
    /// The shortest way to write `q` (written `n`) here: its name without
    /// its module, if this module's scope gives it that name and no local
    /// hides it; else in full.
    fn shortest(&self, n: Symbol, q: Symbol) -> Symbol {
        // stored short: it named nothing in scope (an error, or a hole), keep it
        if n == short(&n) && !self.table.get(&n).is_some_and(|it| it.module.is_empty()) {
            return n;
        }
        let Some(it) = self.table.get(&q) else { return self.shortest_unloaded(n) };
        let short = if it.module.is_empty() { q } else { Symbol::new(&q[it.module.len() + 1..]) };
        let first = Symbol::new(short.split('.').next().unwrap_or(""));
        if self.scope.get(&short) == Some(&q) && !self.is_local(first) {
            return short;
        }
        q
    }
    /// `shortest` for a module file read without the modules it uses (`git
    /// diff` shows one file at a time): `orders.Line` is `Line` if this
    /// module imports it (`use orders.*` or `use orders.{Line}`) and nothing
    /// here has that name. (Two modules imported with `*` that both have a
    /// `Line` can't be told apart without them.)
    fn shortest_unloaded(&self, n: Symbol) -> Symbol {
        let Some((module, short)) = n.rsplit_once('.') else { return n };
        let short = Symbol::new(short);
        let imported = self.uses.iter().any(|(path, items)| path == module && items.as_ref().is_none_or(|is| is.contains(&short)));
        if imported && self.scope.get(&short).is_none() && !self.is_local(short) {
            return short;
        }
        n
    }
    /// The name of a definition being declared, `name` in module `m`.
    fn declared(&self, m: Symbol, name: Symbol) -> Symbol {
        if self.store.is_some() {
            return name;
        }
        self.written(name, qualified(m, name))
    }
    /// A mock named outside tests is an error (`mock MemFiles = { ... }` is
    /// for tests only).
    fn test_only(&mut self, q: Symbol) {
        if !self.for_tests && self.rename.is_none() && self.moves.is_none() && self.table.get(&q).is_some_and(|it| it.mock) && !self.is_local(q) {
            self.errors.push(Diag {
                decl: self.decl.clone(),
                msg: format!("`{}` is a mock: it is for tests only (a `test`, another mock, or an `impl` for one), so a fake never reaches a real run", short(&q)),
            });
        }
    }

    fn lookup(&mut self, n: Symbol) -> Symbol {
        if let Some(&q) = self.scope.get(&n) {
            self.test_only(q);
            return q;
        }
        if !n.contains('.') {
            for &m in self.imports {
                let q = qualified(m, n);
                if self.table.get(&q).is_some_and(|it| !it.public) {
                    self.errors.push(Diag {
                        decl: self.decl.clone(),
                        msg: format!("`{n}` is private to module `{m}` (only `pub` definitions are part of its contract)"),
                    });
                    return q;
                }
            }
        }
        if let Some(it) = self.table.get(&n) {
            let visible = it.public || it.module == self.module || self.module.starts_with(&format!("{}.", it.module));
            if !visible {
                self.errors.push(Diag {
                    decl: self.decl.clone(),
                    msg: format!("`{n}` is private to module `{}` (only `pub` definitions are part of its contract)", it.module),
                });
            } else if it.mock {
                self.test_only(n);
            } else if it.foreign && it.module != self.module && self.rename.is_none() {
                // a foreign function is called only from its own module, which turns
                // it into a capability for the rest of the program
                let m = if it.module.is_empty() { "main" } else { it.module.as_str() };
                self.errors.push(Diag {
                    decl: self.decl.clone(),
                    msg: format!("`{n}` is a foreign function: only module `{m}` can call it (give the rest of the program a capability from `{m}` instead)"),
                });
            }
        }
        if crate::stdlib::is_host(&n) && !crate::stdlib::is_std(&self.module) && self.rename.is_none() {
            self.errors.push(Diag {
                decl: self.decl.clone(),
                msg: format!("`{n}` is internal to the standard library: take a capability instead (`Console` from `std.io`, `Files` from `std.fs`, `Environment` from `std.process`)"),
            });
        }
        n
    }

    /// `orders` or `shop.orders` written as variables and field accesses.
    fn module_path(&self, e: &Expr) -> Option<Symbol> {
        let path: Symbol = match e {
            Expr::Var(x) if !self.is_local(*x) => *x,
            Expr::Field(r, f) => format!("{}.{f}", self.module_path_any(r)?).into(),
            _ => return None,
        };
        self.modules.contains(&path).then_some(path)
    }
    fn module_path_any(&self, e: &Expr) -> Option<String> {
        match e {
            Expr::Var(x) if !self.is_local(*x) => Some(x.to_string()),
            Expr::Field(r, f) => Some(format!("{}.{f}", self.module_path_any(r)?)),
            _ => None,
        }
    }

    fn ty(&mut self, t: &TypeExpr) -> TypeExpr {
        match t {
            TypeExpr::Named(n, args) => {
                let n2 = if n == "Self" || self.tparams.iter().any(|s| s.contains(n)) { *n } else { self.name(*n) };
                TypeExpr::Named(n2, args.iter().map(|a| self.ty(a)).collect())
            }
            TypeExpr::Fn(ps, r) => TypeExpr::Fn(ps.iter().map(|a| self.ty(a)).collect(), Box::new(self.ty(r))),
            TypeExpr::Unit => TypeExpr::Unit,
            TypeExpr::Union(ms) => TypeExpr::Union(ms.iter().map(|m| self.ty(m)).collect()),
            TypeExpr::Refined(b, p) => {
                let b = self.ty(b);
                self.push(&[Symbol::new("it")]);
                let p = self.expr(p);
                self.pop();
                TypeExpr::Refined(Box::new(b), Box::new(p))
            }
        }
    }

    fn pat(&mut self, p: &Pattern) -> Pattern {
        match p {
            Pattern::Bind(x) => {
                self.bind(*x);
                p.clone()
            }
            Pattern::Ctor(c, cp) => {
                let c = self.name(*c);
                let cp = match cp {
                    CtorPat::None => CtorPat::None,
                    CtorPat::Positional(ps) => CtorPat::Positional(ps.iter().map(|x| self.pat(x)).collect()),
                    CtorPat::Type(ps) => CtorPat::Type(ps.iter().map(|x| self.pat(x)).collect()),
                    CtorPat::Record(fs, open) => CtorPat::Record(fs.iter().map(|(f, x)| (f.clone(), self.pat(x))).collect(), *open),
                };
                Pattern::Ctor(c, cp)
            }
            Pattern::List(items, rest) => {
                let items = items.iter().map(|x| self.pat(x)).collect();
                if let Some(r) = rest {
                    self.bind(*r);
                }
                Pattern::List(items, rest.clone())
            }
            Pattern::Tuple(ps) => Pattern::Tuple(ps.iter().map(|x| self.pat(x)).collect()),
            Pattern::Or(ps) => Pattern::Or(ps.iter().map(|x| self.pat(x)).collect()),
            Pattern::Cond(c) => {
                self.push(&[Symbol::new("it")]);
                let c = self.expr(c);
                self.pop();
                Pattern::Cond(Box::new(c))
            }
            // a match comprehension's arm, `p for x in xs`: `xs` uses the
            // module's names; `x` is bound in the arm
            Pattern::For(p, x, xs) => {
                let xs = self.expr(xs);
                let x = self.pat(x);
                let p = match &**p {
                    Pattern::Bind(_) => (**p).clone(),
                    other => self.pat(other),
                };
                Pattern::For(Box::new(p), Box::new(x), Box::new(xs))
            }
            other => other.clone(),
        }
    }

    fn exprs(&mut self, es: &[Expr]) -> Vec<Expr> {
        es.iter().map(|e| self.expr(e)).collect()
    }

    fn expr(&mut self, e: &Expr) -> Expr {
        match e {
            Expr::Var(x) if self.is_local(*x) => e.clone(),
            // another module's definition, written as the parser reads `orders.total`
            Expr::Var(x) if self.store == Some(Store::Shorten) => {
                let n = self.name(*x);
                if n.contains('.') { path_expr(&n) } else { Expr::Var(n) }
            }
            Expr::Var(x) => Expr::Var(self.name(*x)),
            Expr::List(es) => Expr::List(self.exprs(es)),
            Expr::Tuple(es) => Expr::Tuple(self.exprs(es)),
            Expr::Ctor(c, args) => {
                let c = self.name(*c);
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
            Expr::Call(f, args) if matches!(&**f, Expr::Var(x) if PRELUDE_FNS.contains(&x.as_str()) && !self.is_local(*x) && self.scope.get(x).is_none_or(|q| self.methods.contains(q))) => {
                Expr::Call(f.clone(), self.exprs(args))
            }
            // `orders.ship(x)` is read as a method call on the module
            Expr::Call(f, args) if self.store == Some(Store::Shorten) => match self.expr(f) {
                Expr::Field(r, m) if matches!(&**f, Expr::Var(x) if !self.is_local(*x)) => Expr::Method(r, m, self.exprs(args)),
                f => Expr::Call(Box::new(f), self.exprs(args)),
            },
            Expr::Call(f, args) => Expr::Call(Box::new(self.expr(f)), self.exprs(args)),
            Expr::Method(recv, m, args) => {
                if let Some(path) = self.module_path(recv) {
                    // `orders.ship(x)`: a qualified call, not a method
                    let f = self.name(format!("{path}.{m}").into());
                    if self.writing() {
                        // `orders.ship(x)`: a move changes the module written before it
                        let (fp, m) = f.rsplit_once('.').map(|(a, b)| (a.to_string(), b.sym())).unwrap_or((path.to_string(), *m));
                        let recv = if fp == path { recv.clone() } else { Box::new(path_expr(&fp)) };
                        return Expr::Method(recv, m, self.exprs(args));
                    }
                    return Expr::Call(Box::new(Expr::Var(f)), self.exprs(args));
                }
                let recv = self.expr(recv);
                // a method is found from its receiver: stored as written
                let m = if self.is_local(*m) || self.store.is_some() {
                    *m
                } else {
                    let q = self.scope.get(m).copied().unwrap_or(*m);
                    self.written(*m, q)
                };
                Expr::Method(Box::new(recv), m, self.exprs(args))
            }
            Expr::Field(r, f) => {
                if let Some(path) = self.module_path(r) {
                    let q = self.name(format!("{path}.{f}").into());
                    if self.writing() {
                        let (qp, last) = q.rsplit_once('.').map(|(a, b)| (a.to_string(), b.sym())).unwrap_or((path.to_string(), *f));
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
            Expr::Index(x, r) => Expr::Index(Box::new(self.expr(x)), Box::new(self.expr(r))),
            Expr::Dict(tys, kvs) => Expr::Dict(tys.iter().map(|t| self.ty(t)).collect(), kvs.iter().map(|(k, v)| (self.expr(k), self.expr(v))).collect()),
            Expr::Set(tys, xs) => Expr::Set(tys.iter().map(|t| self.ty(t)).collect(), self.exprs(xs)),
            Expr::Comp(kind, elem, gens) => {
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
                Expr::Comp(*kind, Box::new(elem), out)
            }
            Expr::Mut(x) => Expr::Mut(Box::new(self.expr(x))),
            Expr::Try(x, k) => Expr::Try(Box::new(self.expr(x)), *k),
            Expr::Or(x, fb) => Expr::Or(Box::new(self.expr(x)), Box::new(self.expr(fb))),
            Expr::Except(x, name, fb) => {
                let x = self.expr(x);
                self.push(&name.iter().copied().collect::<Vec<_>>());
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
            Expr::Is(x, c) => Expr::Is(Box::new(self.expr(x)), self.name(*c)),
            Expr::If(c, t, f) => Expr::If(Box::new(self.expr(c)), Box::new(self.expr(t)), Box::new(self.expr(f))),
            Expr::Match(s, name, arms) => {
                let s = self.expr(s);
                let arms = arms
                    .iter()
                    .map(|a| {
                        self.push(&name.iter().copied().collect::<Vec<_>>());
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
                            self.bind(*n);
                            Stmt::Var(*n, t, x)
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
                        Stmt::ComptimeFor(p, it, b) => {
                            let it = self.expr(it);
                            self.push(&[]);
                            let p = self.pat(p);
                            let b = self.expr(b);
                            self.pop();
                            Stmt::ComptimeFor(p, it, b)
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
            // compile-time code calls the module's functions like any other
            Expr::Comptime(x) => Expr::Comptime(Box::new(self.expr(x))),
            _ => e.clone(),
        }
    }

    fn bounds(&mut self, bs: &[Bound]) -> Vec<Bound> {
        bs.iter().map(|b| Bound { param: b.param.clone(), traits: b.traits.iter().map(|t| self.ty(t)).collect() }).collect()
    }

    /// A function, an impl method or a trait method; `name` is its resolved name.
    fn func(&mut self, f: &FnDecl, name: Symbol) -> FnDecl {
        self.tparams.push(f.type_params.iter().copied().collect());
        let bounds = self.bounds(&f.bounds);
        let params: Vec<Param> = f.params.iter().map(|p| Param { name: p.name.clone(), ty: self.ty(&p.ty), mutable: p.mutable, comptime: p.comptime }).collect();
        let names: Vec<Symbol> = f.params.iter().map(|p| p.name).collect();
        self.push(&names);
        let ret = self.ty(&f.ret);
        let requires = self.exprs(&f.requires);
        let body = self.expr(&f.body);
        self.push(&[Symbol::new("it")]);
        let ensures = self.exprs(&f.ensures);
        self.pop();
        self.pop();
        self.tparams.pop();
        FnDecl { doc: f.doc.clone(), comments: f.comments.clone(), name, type_params: f.type_params.clone(), bounds, params, ret, requires, ensures, terminates: f.terminates, body, comptime: f.comptime, derived: f.derived }
    }

    fn decl(&mut self, k: &DeclKind) -> DeclKind {
        let m = self.module;
        match k {
            DeclKind::Type(t) => {
                self.tparams.push(t.params.iter().copied().collect());
                let body = match &t.body {
                    TypeBody::Alias(a) => TypeBody::Alias(self.ty(a)),
                    TypeBody::Record(fs) => TypeBody::Record(fs.iter().map(|f| Field { comments: f.comments.clone(), doc: f.doc.clone(), name: f.name.clone(), ty: self.ty(&f.ty), default: f.default.as_ref().map(|x| self.expr(x)) }).collect()),
                    TypeBody::Sum(vs) => TypeBody::Sum(
                        vs.iter()
                            .map(|v| Variant {
                                comments: v.comments.clone(),
                                name: self.declared(m, v.name),
                                payload: match &v.payload {
                                    Payload::None => Payload::None,
                                    Payload::Positional(ts) => Payload::Positional(ts.iter().map(|t| self.ty(t)).collect()),
                                    Payload::Record(fs) => Payload::Record(fs.iter().map(|f| Field { comments: f.comments.clone(), doc: f.doc.clone(), name: f.name.clone(), ty: self.ty(&f.ty), default: f.default.as_ref().map(|x| self.expr(x)) }).collect()),
                                },
                            })
                            .collect(),
                    ),
                };
                self.push(&[Symbol::new("it")]);
                let refinement = t.refinement.as_ref().map(|r| self.expr(r));
                self.pop();
                self.tparams.pop();
                DeclKind::Type(TypeDecl { distinct: t.distinct, name: self.declared(m, t.name), params: t.params.clone(), body, refinement , mock: t.mock })
            }
            DeclKind::Fn(f) => DeclKind::Fn(self.func(f, if self.store.is_none() && !self.writing() { root_fn(m, f.name, true) } else { self.declared(m, f.name) })),
            DeclKind::Extern(e) => DeclKind::Extern(ExternDecl {
                lang: e.lang,
                item: match &e.item {
                    ExternItem::Fn(f) => ExternItem::Fn(self.func(f, self.declared(m, f.name))),
                    ExternItem::Type(t) => ExternItem::Type(self.declared(m, *t)),
                },
            }),
            DeclKind::Test(t) => {
                let name = if m.is_empty() || self.writing() || self.store.is_some() { t.name.clone() } else { format!("{m}: {}", t.name) };
                let params: Vec<Param> = t.params.iter().map(|p| Param { name: p.name, ty: self.ty(&p.ty), mutable: p.mutable, comptime: p.comptime }).collect();
                let names: Vec<Symbol> = t.params.iter().map(|p| p.name).collect();
                self.push(&names);
                let body = self.expr(&t.body);
                self.pop();
                DeclKind::Test(TestDecl { name, params, body })
            }
            DeclKind::Capability(c) => {
                let methods = c.methods.iter().map(|f| self.func(f, self.declared(m, f.name))).collect();
                DeclKind::Capability(CapabilityDecl { name: self.declared(m, c.name), methods })
            }
            DeclKind::Trait(t) => {
                self.tparams.push(t.params.iter().copied().collect());
                let supers = t.supers.iter().map(|s| self.ty(s)).collect();
                let methods = t
                    .methods
                    .iter()
                    .map(|tm| TraitMethod { comments: tm.comments.clone(), doc: tm.doc.clone(), decl: self.func(&tm.decl, self.declared(m, tm.decl.name)), default: tm.default })
                    .collect();
                let laws = t
                    .laws
                    .iter()
                    .map(|l| {
                        let params: Vec<Param> = l.params.iter().map(|p| Param { name: p.name.clone(), ty: self.ty(&p.ty), mutable: p.mutable, comptime: p.comptime }).collect();
                        let names: Vec<Symbol> = l.params.iter().map(|p| p.name).collect();
                        self.push(&names);
                        let body = self.expr(&l.body);
                        self.pop();
                        Law { comments: l.comments.clone(), doc: l.doc.clone(), name: l.name.clone(), params, body }
                    })
                    .collect();
                let derivable = t.derivable.as_ref().map(|d| Derivable { kind: self.ty(&d.kind), requires: d.requires.iter().map(|r| self.expr(r)).collect() });
                self.tparams.pop();
                DeclKind::Trait(TraitDecl { name: self.declared(m, t.name), params: t.params.clone(), supers, methods, laws, derivable, capability: t.capability })
            }
            DeclKind::Impl(i) => {
                self.tparams.push(i.type_params.iter().copied().collect());
                let bounds = self.bounds(&i.bounds);
                let trait_q = if i.is_inherent() { Symbol::EMPTY } else { self.lookup(i.trait_name) };
                let trait_name = if i.is_inherent() { Symbol::EMPTY } else { self.written(i.trait_name, trait_q) };
                let trait_args = i.trait_args.iter().map(|a| self.ty(a)).collect();
                let for_type = self.ty(&i.for_type);
                if i.is_inherent() {
                    let head = crate::types::texpr_head(&for_type);
                    let short = short(&head).to_string();
                    if head.contains('.') && module_of(&head) != m.as_str() {
                        self.errors.push(Diag { decl: self.decl.clone(), msg: format!("`impl {short}` must be in the module that defines `{short}` (`{}`)", module_of(&head)) });
                    }
                }
                // impl methods are named like the trait's methods (in the trait's module,
                // whatever the trait is called here); a type's own, like the type
                let tm = if i.is_inherent() { m } else { Symbol::new(module_of(&trait_q)) };
                let methods = i.methods.iter().map(|f| self.func(f, self.declared(tm, f.name))).collect();
                self.tparams.pop();
                DeclKind::Impl(ImplDecl { type_params: i.type_params.clone(), bounds, trait_name, trait_args, for_type, methods, derived: i.derived })
            }
            DeclKind::Use(u) => DeclKind::Use(u.clone()),
        }
    }
}

/// The names a move changes: the definition, and its variants or methods
/// (named in their module): old qualified name → new qualified name.
fn moved_names(p: &Program, old: &str, target: &str) -> Result<HashMap<String, String>, String> {
    let d = p.decls.iter().find(|d| d.qname() == old).ok_or(format!("no definition `{old}`"))?;
    let mut out = HashMap::default();
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
                            items.push(Symbol::from(item));
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
    p.decls.insert(at, Decl { comments: vec![], doc: vec![], kind: DeclKind::Use(UseDecl { path: Symbol::from(path), items: Some(vec![Symbol::from(item)]) }), module: Symbol::from(m), public: false });
}

/// Moving `old` into module `target` (no checks): the definition changes
/// module, references written with its module follow it, short ones stay
/// short and their module imports it, and `use` items that named it go.
/// Returns the program and the definition's new qualified name.
fn move_apply(p: &Program, old: &str, target: &str) -> Result<(Program, String), String> {
    let moves = moved_names(p, old, target)?;
    let new_q = moves[old].clone();
    let i = p.decls.iter().position(|d| d.qname() == old).unwrap();
    let (out, _) = run_with(p, None, Some(&moves), None);
    // references are stored in full, so they follow the move; imports are for
    // the text: where it was written short, the new module is imported
    let written_p = written_form(p);
    let (_, needs) = run_with(&written_p, None, Some(&moves), None);
    let mut program = out.program;
    for (r, d) in program.decls.iter_mut().zip(&p.decls) {
        r.module = d.module.clone();
    }
    program.decls[i].module = Symbol::from(target);
    // what the moved definition uses from its old module (or that module's imports)
    // must be imported where it goes: names it writes short
    let written = names_in(&written_p.decls[i].kind);
    let deps = crate::store::graph(p).get(old).cloned().unwrap_or_default();
    let mut imports: Vec<(String, String, String)> = Vec::new();
    for dep in deps {
        let m = module_of(&dep).to_string();
        let short = short(&dep).to_string();
        if m != target && !m.is_empty() && !moves.contains_key(&dep) && written.contains(&short.sym()) {
            imports.push((target.to_string(), m, short));
        }
    }
    // modules that write a moved name short import it from its new module
    for (k, ns) in needs.iter().enumerate() {
        if k == i {
            continue; // the moved definition's own references (recursion) are local where it goes
        }
        for nq in ns {
            let m = p.decls[k].module.to_string();
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
    back.decls[i].module = source.into();
    let moves = moved_names(&back, old, &target).ok()?;
    let (program, _) = move_apply(&back, old, &target).ok()?;
    let by_name = |p: &Program| -> HashMap<String, Decl> { resolve(p).program.decls.into_iter().zip(&p.decls).filter(|(_, d)| !matches!(d.kind, DeclKind::Use(_))).map(|(r, d)| (d.qname(), r)).collect() };
    let (rm, rb, ra) = (by_name(merged), by_name(&back), by_name(&program));
    let tb: HashMap<String, Decl> = back.decls.iter().filter(|d| !matches!(d.kind, DeclKind::Use(_))).map(|d| (d.qname(), d.clone())).collect();
    for d in program.decls.iter().filter(|d| !matches!(d.kind, DeclKind::Use(_))) {
        let q = d.qname();
        let touched = q != new && tb.get(&q).is_some_and(|b| b != d);
        let moved_meaning = || rb.get(&q).map(|b| renamed(b, &moves));
        let got = ra.get(&q)?;
        // a definition the follow changed means what it meant before the move,
        // moved; one it didn't change means the same as in the merge, or (a
        // use the merge broke, `charge(o)` written short, its import fixed)
        // what it meant before the move, moved
        let ok = if touched { moved_meaning().as_ref() == Some(got) } else { rm.get(&q) == Some(got) || moved_meaning().as_ref() == Some(got) };
        if !ok {
            return None;
        }
    }
    // the definitions, or only a use list (an import the other side's code needs)
    let changed = program.decls.len() != merged.decls.len() || program.decls.iter().zip(&merged.decls).any(|(a, b)| a != b);
    changed.then_some(program)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every example, and the standard library, as stored.
    fn examples() -> Vec<(String, Program)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut paths: Vec<_> = std::fs::read_dir(root.join("examples")).unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir() || p.extension().is_some_and(|e| e == "strata")).collect();
        paths.push(root.join("std"));
        paths.push(root.join("tests/data/sugar.strata"));
        paths.sort();
        paths.into_iter().map(|p| (p.display().to_string(), crate::package::program(&p).unwrap())).collect()
    }

    fn text(p: &Program) -> String {
        let mut mods: Vec<Symbol> = p.decls.iter().map(|d| d.module).collect();
        mods.dedup();
        mods.iter().map(|m| crate::printer::module_file(p, m)).collect::<Vec<_>>().join("\n// ---\n")
    }

    #[test]
    fn stored_and_written_forms_round_trip() {
        for (name, stored) in examples() {
            // loaded as stored: storing again changes nothing
            assert_eq!(stored_form(&stored), stored, "{name}: not in stored form");
            let written = written_form(&stored);
            // storing what is shown stores the same tree
            assert_eq!(stored_form(&written), stored, "{name}: stored form is not stable");
            // what is shown is what the parser reads from its text
            for m in written.decls.iter().map(|d| d.module).collect::<std::collections::BTreeSet<_>>() {
                let own = Program { decls: written.decls.iter().filter(|d| d.module == m).cloned().collect() };
                let reparsed = crate::parser::parse_module_file(&crate::printer::module_file(&written, &m), &m).unwrap();
                assert_eq!(reparsed, own, "{name}: module {m} doesn't read back as shown");
            }
            if name.ends_with("tasks") {
                let t = text(&stored);
                assert!(t.contains("core.Task") && t != text(&written), "references are stored qualified");
            }
        }
    }
}

