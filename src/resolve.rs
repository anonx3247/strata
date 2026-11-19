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

use crate::ast::*;
use crate::types::Diag;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
struct Item {
    module: String,
    public: bool,
}

fn qualify(module: &str, name: &str) -> String {
    if module.is_empty() { name.to_string() } else { format!("{module}.{name}") }
}

fn module_of(qname: &str) -> &str {
    qname.rsplit_once('.').map(|(m, _)| m).unwrap_or("")
}

pub struct Resolution {
    pub program: Program,
    pub errors: Vec<Diag>,
}

pub fn resolve(p: &Program) -> Resolution {
    // Programs without modules or imports are already resolved.
    if p.decls.iter().all(|d| d.module.is_empty() && !matches!(d.kind, DeclKind::Use(_))) {
        return Resolution { program: p.clone(), errors: vec![] };
    }
    run(p, None)
}

/// Rename the definition `old` (a qualified name) to `new` (a short name in
/// the same module), and every reference to it. Names are looked up exactly
/// as the checker does (locals shadow globals, `use` brings names in), and
/// everything else is written back as it was.
pub fn rename(p: &Program, old: &str, new: &str) -> Result<Program, String> {
    let d = p.decls.iter().find(|d| d.qname() == old).ok_or(format!("no definition `{old}`"))?;
    if !matches!(d.kind, DeclKind::Fn(_) | DeclKind::Type(_) | DeclKind::Trait(_)) {
        return Err(format!("only functions, types and traits can be renamed (`{old}` is a {})", d.kind_str()));
    }
    let valid = new.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && new.chars().all(|c| c.is_alphanumeric() || c == '_');
    if !valid {
        return Err(format!("`{new}` is not a name (give the new short name, without the module)"));
    }
    let upper = |s: &str| s.chars().next().is_some_and(|c| c.is_uppercase());
    if upper(&d.name()) != upper(new) {
        return Err(format!("`{new}` must start with {} like `{}`", if upper(new) { "a lowercase letter" } else { "an uppercase letter" }, d.name()));
    }
    let new_q = if d.module.is_empty() { new.to_string() } else { format!("{}.{new}", d.module) };
    if p.decls.iter().any(|x| x.qname() == new_q) {
        return Err(format!("`{new_q}` already exists"));
    }
    let out = run(p, Some((old.to_string(), new.to_string())));
    let mut program = out.program;
    for (r, d) in program.decls.iter_mut().zip(&p.decls) {
        r.module = d.module.clone();
    }
    // the rename must not change what any other name refers to
    // (`use` lists are compared by the names they bring in, which the resolution already covers)
    let resolved = |p: &Program| {
        let mut r = resolve(p).program;
        r.decls.retain(|d| !matches!(d.kind, DeclKind::Use(_)));
        serde_json::to_string(&r).unwrap()
    };
    let (before, after) = (resolved(p), resolved(&program));
    let expect = before.replace(&format!("\"{old}\""), &format!("\"{new_q}\""));
    if after != expect {
        return Err(format!("renaming `{old}` to `{new}` would change what other names refer to (something visible there is already called `{new}`)"));
    }
    Ok(program)
}

fn run(p: &Program, rename: Option<(String, String)>) -> Resolution {
    let mut table: HashMap<String, Item> = HashMap::new();
    // children of a type (its constructors) or a trait (its methods), as (short, qualified)
    let mut children: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut modules: HashSet<String> = HashSet::new();
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
                    children.entry(q.clone()).or_default().push((m.decl.name.clone(), mq));
                }
            }
            DeclKind::Fn(_) => {
                table.insert(qualify(&d.module, &d.name()), item);
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
    for d in &p.decls {
        let mut r = Rw { scope: &scopes[&d.module], table: &table, modules: &modules, module: &d.module, imports: &imports[&d.module], decl: d.qname(), locals: vec![], tparams: vec![], errors: vec![], rename: rename.as_ref() };
        let kind = match (&d.kind, rename.as_ref()) {
            // `use core.{same_ids}` names what it imports
            (DeclKind::Use(u), Some((old, new))) => DeclKind::Use(UseDecl {
                path: u.path.clone(),
                items: u.items.as_ref().map(|is| is.iter().map(|i| if &format!("{}.{i}", u.path) == old { new.clone() } else { i.clone() }).collect()),
            }),
            _ => r.decl(&d.kind),
        };
        errors.extend(r.errors);
        decls.push(Decl { comments: d.comments.clone(), doc: d.doc.clone(), kind, module: String::new(), public: d.public });
    }
    Resolution { program: Program { decls }, errors }
}

struct Rw<'a> {
    scope: &'a HashMap<String, String>,
    table: &'a HashMap<String, Item>,
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
    fn written(&self, n: &str, q: &str) -> String {
        match self.rename {
            None => q.to_string(),
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
            Expr::Call(f, args) => Expr::Call(Box::new(self.expr(f)), self.exprs(args)),
            Expr::Method(recv, m, args) => {
                if let Some(path) = self.module_path(recv) {
                    // `orders.ship(x)`: a qualified call, not a method
                    let f = self.name(&format!("{path}.{m}"));
                    if self.rename.is_some() {
                        let m = f.rsplit('.').next().unwrap_or(m).to_string();
                        return Expr::Method(recv.clone(), m, self.exprs(args));
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
                    if self.rename.is_some() {
                        return Expr::Field(r.clone(), q.rsplit('.').next().unwrap_or(f).to_string());
                    }
                    return Expr::Var(q);
                }
                Expr::Field(Box::new(self.expr(r)), f.clone())
            }
            Expr::Mock(t) => Expr::Mock(self.ty(t)),
            Expr::Return(x) => Expr::Return(Box::new(self.expr(x))),
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
            Expr::Match(s, arms) => {
                let s = self.expr(s);
                let arms = arms
                    .iter()
                    .map(|a| {
                        self.push(&[]);
                        let pat = self.pat(&a.pat);
                        let body = self.expr(&a.body);
                        self.pop();
                        Arm { comments: a.comments.clone(), pat, body }
                    })
                    .collect();
                Expr::Match(Box::new(s), arms)
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
        FnDecl { doc: f.doc.clone(), comments: f.comments.clone(), name, type_params: f.type_params.clone(), bounds, params, ret, requires, ensures, body }
    }

    fn decl(&mut self, k: &DeclKind) -> DeclKind {
        let m = self.module.to_string();
        match k {
            DeclKind::Type(t) => {
                self.tparams.push(t.params.iter().cloned().collect());
                let body = match &t.body {
                    TypeBody::Alias(a) => TypeBody::Alias(self.ty(a)),
                    TypeBody::Record(fs) => TypeBody::Record(fs.iter().map(|f| Field { comments: f.comments.clone(), name: f.name.clone(), ty: self.ty(&f.ty) }).collect()),
                    TypeBody::Sum(vs) => TypeBody::Sum(
                        vs.iter()
                            .map(|v| Variant {
                                comments: v.comments.clone(),
                                name: self.declared(&m, &v.name),
                                payload: match &v.payload {
                                    Payload::None => Payload::None,
                                    Payload::Positional(ts) => Payload::Positional(ts.iter().map(|t| self.ty(t)).collect()),
                                    Payload::Record(fs) => Payload::Record(fs.iter().map(|f| Field { comments: f.comments.clone(), name: f.name.clone(), ty: self.ty(&f.ty) }).collect()),
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
                let name = if m.is_empty() || self.rename.is_some() { t.name.clone() } else { format!("{m}: {}", t.name) };
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
                        Law { name: l.name.clone(), params, body }
                    })
                    .collect();
                self.tparams.pop();
                DeclKind::Trait(TraitDecl { name: self.declared(&m, &t.name), params: t.params.clone(), supers, methods, laws })
            }
            DeclKind::Impl(i) => {
                self.tparams.push(i.type_params.iter().cloned().collect());
                let bounds = self.bounds(&i.bounds);
                let trait_name = self.name(&i.trait_name);
                let trait_args = i.trait_args.iter().map(|a| self.ty(a)).collect();
                let for_type = self.ty(&i.for_type);
                // impl methods are named like the trait's methods
                let tm = module_of(&trait_name).to_string();
                let methods = i.methods.iter().map(|f| self.func(f, self.declared(&tm, &f.name))).collect();
                self.tparams.pop();
                DeclKind::Impl(ImplDecl { type_params: i.type_params.clone(), bounds, trait_name, trait_args, for_type, methods })
            }
            DeclKind::Use(u) => DeclKind::Use(u.clone()),
        }
    }
}
