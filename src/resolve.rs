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
    let short = q.rsplit('.').next().unwrap_or(q).to_string();
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

/// Rename field `field` of `owner` (a record type or record variant) to `new`.
/// Field accesses (`x.qty`) aren't resolved by name but by type, so this
/// renames a field only when no other record in the program (or the
/// prelude) has a field of that name: then every `.qty`, record literal,
/// update and pattern field called `qty` is this one. The caller type-checks
/// the result.
fn rename_field(p: &Program, owner: &str, field: &str, new: &str) -> Result<Program, String> {
    if fields_of(p, owner).iter().any(|f| f == new) {
        return Err(format!("`{owner}` already has a field `{new}`"));
    }
    // every record that has a field of that name, besides the owner
    let prelude = crate::parser::parse_program(crate::types::PRELUDE).unwrap_or_default();
    let mut others: Vec<String> = Vec::new();
    for q in p.decls.iter().chain(&prelude.decls).flat_map(|d| match &d.kind {
        DeclKind::Type(t) => match &t.body {
            TypeBody::Record(_) => vec![qualify(&d.module, &t.name)],
            TypeBody::Sum(vs) => vs.iter().map(|v| qualify(&d.module, &v.name)).collect(),
            _ => vec![],
        },
        _ => vec![],
    }) {
        if q != owner && (fields_of(p, &q).iter().any(|f| f == field) || fields_of(&prelude, &q).iter().any(|f| f == field)) {
            others.push(q);
        }
    }
    if field == "value" && p.decls.iter().any(|d| matches!(&d.kind, DeclKind::Type(t) if t.distinct)) {
        others.push("every distinct type (`.value`)".into());
    }
    if !others.is_empty() {
        return Err(format!(
            "`{field}` is also a field of {}, and `x.{field}` is told apart by type, so it can't be renamed by name yet",
            others.iter().map(|o| format!("`{o}`")).collect::<Vec<_>>().join(", ")
        ));
    }
    let mut modules: HashSet<String> = HashSet::new();
    for d in &p.decls {
        let mut m = d.module.as_str();
        while !m.is_empty() {
            modules.insert(m.to_string());
            m = module_of(m);
        }
    }
    let mut out = p.clone();
    for d in &mut out.decls {
        // the declaration
        if let DeclKind::Type(t) = &mut d.kind {
            let tq = qualify(&d.module, &t.name);
            match &mut t.body {
                TypeBody::Record(fs) if tq == owner => fs.iter_mut().filter(|f| f.name == field).for_each(|f| f.name = new.to_string()),
                TypeBody::Sum(vs) => {
                    for v in vs.iter_mut().filter(|v| qualify(&d.module, &v.name) == owner) {
                        if let Payload::Record(fs) = &mut v.payload {
                            fs.iter_mut().filter(|f| f.name == field).for_each(|f| f.name = new.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
        // its uses
        let mut j = serde_json::to_value(&d.kind).map_err(|e| e.to_string())?;
        rename_field_uses(&mut j, field, new, &modules);
        d.kind = serde_json::from_value(j).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// The dotted path an expression spells, if it is only names: `shop.orders`.
fn json_path(e: &serde_json::Value) -> Option<String> {
    if let Some(x) = e.get("Var").and_then(|v| v.as_str()) {
        return Some(x.to_string());
    }
    let a = e.get("Field")?.as_array()?;
    Some(format!("{}.{}", json_path(&a[0])?, a[1].as_str()?))
}

fn rename_field_uses(v: &mut serde_json::Value, field: &str, new: &str, modules: &HashSet<String>) {
    use serde_json::Value as J;
    let rename_pairs = |pairs: &mut J| {
        for pair in pairs.as_array_mut().into_iter().flatten() {
            if let Some(a) = pair.as_array_mut() {
                if a.len() == 2 && a[0].as_str() == Some(field) {
                    a[0] = J::String(new.to_string());
                }
            }
        }
    };
    if let J::Object(o) = v {
        if o.len() == 1 {
            if let Some(J::Array(a)) = o.get_mut("Field") {
                // `x.qty`, but not `orders.qty` (a qualified name)
                if a.len() == 2 && a[1].as_str() == Some(field) && !json_path(&a[0]).is_some_and(|m| modules.contains(&m)) {
                    a[1] = J::String(new.to_string());
                }
            }
            if let Some(J::Array(a)) = o.get_mut("Record") {
                if a.len() == 2 && a[0].is_array() && a[1].is_boolean() {
                    rename_pairs(&mut a[0]); // a pattern: `Line { qty, .. }`
                } else if a.iter().all(|x| x.as_array().is_some_and(|p| p.len() == 2 && p[0].is_string())) {
                    rename_pairs(&mut o["Record"]); // a literal: `Line { qty: 1 }`
                }
            }
            if let Some(J::Array(a)) = o.get_mut("Update") {
                if a.len() == 2 {
                    rename_pairs(&mut a[1]); // `{ ..line, qty: 2 }`
                }
            }
        }
        for (_, x) in o.iter_mut() {
            rename_field_uses(x, field, new, modules);
        }
    } else if let J::Array(a) = v {
        for x in a {
            rename_field_uses(x, field, new, modules);
        }
    }
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
    let short = old.rsplit('.').next().unwrap_or(old);
    let upper = |s: &str| s.chars().next().is_some_and(|c| c.is_uppercase());
    if upper(short) != upper(new) {
        return Err(format!("`{new}` must start with {} like `{short}`", if upper(new) { "a lowercase letter" } else { "an uppercase letter" }));
    }
    if kind == "field" {
        return rename_field(p, &module, short, new);
    }
    let new_q = qualify(&module, new);
    if defined(p).contains(&new_q) {
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
    let module = old.rsplit_once('.').map(|(m, _)| m).unwrap_or("");
    let new_q = if module.is_empty() { new.to_string() } else { format!("{module}.{new}") };
    let each = |p: &Program| resolve(p).program.decls.iter().map(|d| serde_json::to_string(d).unwrap()).collect::<Vec<_>>();
    let (rm, rb, ra) = (each(merged), each(back), each(&program));
    for (i, d) in program.decls.iter().enumerate() {
        if matches!(d.kind, DeclKind::Use(_)) {
            continue;
        }
        // the renamed definition, and what the rename didn't touch, are as merged
        let touched = program.decls[i] != back.decls[i] && back.decls[i].qname() != old;
        let expect = if touched { rb[i].replace(&format!("\"{old}\""), &format!("\"{new_q}\"")) } else { rm[i].clone() };
        if ra[i] != expect {
            return Err(format!("following the rename of `{old}` to `{new}` would change what `{}` refers to", d.qname()));
        }
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
            Expr::Interp(parts) => Expr::Interp(
                parts
                    .iter()
                    .map(|p| match p {
                        StrPart::Lit(l) => StrPart::Lit(l.clone()),
                        StrPart::Expr(e) => StrPart::Expr(self.expr(e)),
                    })
                    .collect(),
            ),
            Expr::Return(x) => Expr::Return(Box::new(self.expr(x))),
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
                        Law { comments: l.comments.clone(), doc: l.doc.clone(), name: l.name.clone(), params, body }
                    })
                    .collect();
                self.tparams.pop();
                DeclKind::Trait(TraitDecl { name: self.declared(&m, &t.name), params: t.params.clone(), supers, methods, laws })
            }
            DeclKind::Impl(i) => {
                self.tparams.push(i.type_params.iter().cloned().collect());
                let bounds = self.bounds(&i.bounds);
                let trait_q = self.lookup(&i.trait_name);
                let trait_name = self.written(&i.trait_name, &trait_q);
                let trait_args = i.trait_args.iter().map(|a| self.ty(a)).collect();
                let for_type = self.ty(&i.for_type);
                // impl methods are named like the trait's methods (in the trait's module,
                // whatever the trait is called here)
                let tm = module_of(&trait_q).to_string();
                let methods = i.methods.iter().map(|f| self.func(f, self.declared(&tm, &f.name))).collect();
                self.tparams.pop();
                DeclKind::Impl(ImplDecl { type_params: i.type_params.clone(), bounds, trait_name, trait_args, for_type, methods })
            }
            DeclKind::Use(u) => DeclKind::Use(u.clone()),
        }
    }
}
