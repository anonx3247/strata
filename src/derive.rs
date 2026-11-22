//! Derived traits: each `derive Trait for Type` becomes an ordinary `impl`,
//! made once the program is resolved, so nothing that checks, runs, verifies
//! or compiles code sees a `derive` (IDEAS.md, "Derived traits, `comptime`
//! and special methods").
//!
//! For `derive CommandLine for Options`:
//! 1. `Options` is described as a value of `std.typing` (`Record`, or `Sum`
//!    for a sum type), and checked against the trait's kind
//!    (`derivable(t: Record)`; a kind may be a refinement, as `Choice` is)
//!    and its `requires`, by running them;
//! 2. each `derived fn` of the trait is specialized: `t` is that value,
//!    `comptime e` is evaluated and its value written in its place,
//!    `t.make(f -> e)` / `t.try_make(f -> e)` become the construction of an
//!    `Options`, field by field, and a call of a function with `comptime`
//!    parameters becomes a call of a copy made for those values, where
//!    `f.type` is the field's type and `match comptime` keeps one arm;
//! 3. the result is `impl CommandLine for Options { ... }` with those methods
//!    and the ones written by hand, and the copies are added to the program.
//!
//! Compile-time code is mostly data (a description, a known name, a field
//! of one), read as it is; the rest is compiled and run on the JIT, within
//! a step budget, its value read back from memory (`llvm::native`).

use crate::ast::*;
use crate::symbol::Symbol;
use crate::types::{Diag, Env};
use std::cell::RefCell;
use std::collections::HashMap;

/// The most steps compile-time code may take, for one `derive`.
const BUDGET: u64 = 5_000_000;

/// Replace each `derive` by its `impl` (or report why it can't be made).
pub fn expand(p: &mut Program, errors: &mut Vec<Diag>) {
    settings(p);
    derive_all(p, errors);
    fold_comptime(p, errors);
}

fn derive_all(p: &mut Program, errors: &mut Vec<Diag>) {
    if !p.decls.iter().any(|d| matches!(&d.kind, DeclKind::Impl(i) if i.derived)) {
        return;
    }
    let (env, _) = Env::build(p);
    let mut extra: Vec<Decl> = Vec::new();
    let mut keep = vec![true; p.decls.len()];
    let snapshot = p.clone();
    for (k, d) in p.decls.iter_mut().enumerate() {
        let DeclKind::Impl(i) = &d.kind else { continue };
        if !i.derived {
            continue;
        }
        let name = d.name();
        let mut x = Expander::new(&env, &snapshot);
        match x.derive(i) {
            Ok(imp) => {
                d.kind = DeclKind::Impl(imp);
                extra.extend(x.copies.into_iter().map(|f| Decl { comments: vec![], doc: vec![], kind: DeclKind::Fn(f), module: d.module, public: false }));
            }
            Err(msg) => {
                errors.push(Diag { decl: name, msg });
                keep[k] = false;
            }
        }
    }
    let mut k = 0;
    p.decls.retain(|_| {
        k += 1;
        keep[k - 1]
    });
    p.decls.extend(extra);
}

/// The values of `es` (expressions of `prog`'s definitions, a resolved
/// program), computed as compile-time code is (on the JIT, cached), each
/// written as a literal; or why one can't be.
pub fn comptime_literals(prog: &Program, es: &[Expr]) -> Vec<Res<Expr>> {
    let (env, _) = Env::build(prog);
    es.iter()
        .map(|e| {
            let mut ex = Expander::new(&env, prog);
            ex.eval_as(e, e).and_then(|v| ex.literal(&v))
        })
        .collect()
}

/// A function made once per compile-time value (`comptime fn`, `comptime`
/// parameters), specialized where it is called: its `comptime` code is
/// evaluated there, not here.
fn is_template(f: &FnDecl) -> bool {
    f.comptime || f.derived || f.params.iter().any(|x| x.comptime)
}

/// `comptime e` in ordinary code (outside a `derive`): evaluated while
/// compiling, as a derive's, and its value written in as a literal, so
/// `comptime parse("[a-z]+")` is a constant in the program. `e` may use
/// definitions, not the variables around it.
fn fold_comptime(p: &mut Program, errors: &mut Vec<Diag>) {
    let has = |f: &FnDecl| !is_template(f) && any_expr(&f.body, |e| matches!(e, Expr::Comptime(_)));
    let wanted = p.decls.iter().any(|d| match &d.kind {
        DeclKind::Fn(f) => has(f),
        DeclKind::Impl(i) => i.methods.iter().any(has),
        DeclKind::Test(_) => {
            let mut found = false;
            let mut k = d.kind.clone();
            decl_exprs_mut(&mut k, |e| found = found || matches!(e, Expr::Comptime(_)));
            found
        }
        _ => false,
    });
    if !wanted {
        return;
    }
    // evaluating one resolves the program made for it (`cache_key`), which
    // would come back here: that program holds no `comptime` to fold
    // (`compiled` leaves out what holds one), and this is the backstop
    thread_local!(static FOLDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
    if FOLDING.with(|f| f.replace(true)) {
        return;
    }
    let (env, _) = Env::build(p);
    let snapshot = p.clone();
    let fold = |k: &mut DeclKind, errs: &mut Vec<String>| {
        decl_exprs_mut(k, |e| {
            let Expr::Comptime(x) = &*e else { return };
            let mut ex = Expander::new(&env, &snapshot);
            match ex.eval_as(x, x).and_then(|v| ex.literal(&v)) {
                Ok(lit) => *e = lit,
                Err(m) if m.contains("unknown variable") => errs.push(format!("{m}: compile-time code can use definitions, not the variables around it")),
                Err(m) => errs.push(m),
            }
        })
    };
    for d in p.decls.iter_mut() {
        let name = d.name();
        let mut errs = vec![];
        match &mut d.kind {
            DeclKind::Fn(f) if !is_template(f) => fold(&mut d.kind, &mut errs),
            DeclKind::Test(_) => fold(&mut d.kind, &mut errs),
            DeclKind::Impl(i) => {
                for m in i.methods.iter_mut().filter(|m| !is_template(m)) {
                    let mut k = DeclKind::Fn(m.clone());
                    fold(&mut k, &mut errs);
                    let DeclKind::Fn(f) = k else { unreachable!() };
                    *m = f;
                }
            }
            _ => {}
        }
        errors.extend(errs.into_iter().map(|msg| Diag { decl: name.clone(), msg }));
    }
    FOLDING.with(|f| f.set(false));
}

/// `setting("json.unknown_keys")` anywhere: the package's setting, written
/// in as a constant (`Some("error")` or `None`), so it is known when the
/// code runs, compiled or not.
fn settings(p: &mut Program) {
    for d in &mut p.decls {
        decl_exprs_mut_post(&mut d.kind, |e| {
            let Expr::Call(f, args) = &*e else { return };
            if !matches!(&**f, Expr::Var(n) if n.as_str() == "std.typing.setting") {
                return;
            }
            let [Expr::Str(key)] = args.as_slice() else { return };
            *e = match crate::package::setting(key) {
                Some(v) => Expr::Ctor(Symbol::new("Some"), CtorArgs::Positional(vec![Expr::Str(v)])),
                None => Expr::Ctor(Symbol::new("None"), CtorArgs::None),
            };
        });
    }
}

type Res<X> = Result<X, String>;

/// Where compile-time values are kept between runs: they are pure and
/// deterministic, so a value is the same wherever it was computed (shared by
/// every program, as Go's build cache; `STRATA_CACHE` puts it elsewhere).
fn cache_dir() -> std::path::PathBuf {
    match std::env::var_os("STRATA_CACHE") {
        Some(d) => std::path::PathBuf::from(d).join("comptime"),
        None => std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache/strata/comptime"),
    }
}

/// The key of compile-time function `name` of `p`: its code and everything
/// it reaches (`closure_hash`), and this strata (a new one may compile it
/// differently).
fn cache_key(p: &Program, name: &str) -> Option<String> {
    use sha2::{Digest, Sha256};
    let exe = std::env::current_exe().ok()?;
    let meta = std::fs::metadata(&exe).ok()?;
    let stamp = format!("{:?}{}", meta.modified().ok()?, meta.len());
    let closure = crate::store::closure_hash(p, name);
    let digest = Sha256::digest(format!("{stamp}\n{closure}").as_bytes());
    Some(digest.iter().map(|b| format!("{b:02x}")).collect())
}

struct Expander<'a> {
    env: &'a Env,
    prog: &'a Program,

    /// Values known at compile time, by name (`t`, a `comptime` parameter).
    known: HashMap<Symbol, Expr>,
    /// Types known at compile time, by name (`f.type`, `x` in `Option[x] =>`).
    types: HashMap<String, TypeExpr>,
    /// The type behind each `Type` value made (by its JSON).
    written: RefCell<HashMap<String, TypeExpr>>,
    /// The type derived for.
    self_ty: TypeExpr,
    /// Copies of functions with `comptime` parameters, made for their values.
    copies: Vec<FnDecl>,
    made: HashMap<String, Symbol>,
    /// A variant's constructor, qualified (by its `Type` value's JSON).
    ctors: RefCell<HashMap<String, String>>,
    /// Names bound inside the code being specialized, which hide known ones.
    hidden: Vec<Vec<Symbol>>,
    /// The type parameters of the type derived for (`T` of `Pair[T]`).
    params: Vec<Symbol>,
    /// The trait being derived, and its methods.
    trait_name: Symbol,
    trait_methods: Vec<Symbol>,
    /// The default of each field (by its `Field` value's JSON).
    defaults: RefCell<HashMap<String, Expr>>,
    /// In a match comprehension's arm: the matched expression (as text) and
    /// the names its variant's fields are bound to, for `f.of(x)`.
    arm_fields: Vec<(String, Vec<(String, Symbol)>)>,
}

fn q(name: &str) -> String {
    format!("std.typing.{name}")
}

fn record(name: &str, fields: Vec<(&str, Expr)>) -> Expr {
    Expr::Ctor(Symbol::new(&q(name)), CtorArgs::Record(fields.into_iter().map(|(k, v)| (Symbol::new(k), v)).collect()))
}

/// A constructor without fields (`None`, `std.typing.Record`).
fn atom(name: &str) -> Expr {
    Expr::Ctor(Symbol::new(name), CtorArgs::None)
}

/// `Self` used as a value (`Self.fields`, `match comptime Self`): the
/// type derived for.
fn is_self(e: &Expr) -> bool {
    matches!(e, Expr::Var(x) | Expr::Ctor(x, CtorArgs::None) if x == "Self")
}

fn some(v: Expr) -> Expr {
    Expr::Ctor(Symbol::new("Some"), CtorArgs::Positional(vec![v]))
}

fn none() -> Expr {
    atom("None")
}

/// The shapes of `std.typing.Kind`.
const KINDS: [&str; 8] = ["Record", "Tuple", "Union", "Function", "Atom", "Unit", "Builtin", "Param"];

/// A field of a record value (`Type`, `Field`).
fn get<'v>(v: &'v Expr, key: &str) -> Option<&'v Expr> {
    match v {
        Expr::Ctor(_, CtorArgs::Record(fs)) => fs.iter().find(|(k, _)| k == key).map(|(_, x)| x),
        _ => None,
    }
}

fn set(v: &mut Expr, key: &str, x: Expr) {
    if let Expr::Ctor(_, CtorArgs::Record(fs)) = v {
        if let Some(f) = fs.iter_mut().find(|(k, _)| k == key) {
            f.1 = x;
        }
    }
}

/// A `Type` value's kind (`"Record"`, `"Atom"`, ..).
fn kind_name(v: &Expr) -> Option<String> {
    match get(v, "kind")? {
        Expr::Ctor(k, CtorArgs::None) => Some(crate::names::short(k).to_string()),
        _ => None,
    }
}

fn str_of(v: &Expr, key: &str) -> Option<String> {
    match get(v, key)? {
        Expr::Str(s) => Some(s.clone()),
        Expr::Ctor(_, CtorArgs::Positional(xs)) => match xs.first() {
            Some(Expr::Str(s)) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn strs(xs: &[String]) -> Expr {
    Expr::List(xs.iter().map(|s| Expr::Str(s.clone())).collect())
}

/// Whether a type mentions the name `p` (a type parameter).
fn mentions_name(t: &TypeExpr, p: Symbol) -> bool {
    match t {
        TypeExpr::Named(n, args) => *n == p || args.iter().any(|a| mentions_name(a, p)),
        TypeExpr::Refined(b, _) => mentions_name(b, p),
        TypeExpr::Fn(ps, r) => ps.iter().any(|a| mentions_name(a, p)) || mentions_name(r, p),
        TypeExpr::Union(ms) => ms.iter().any(|a| mentions_name(a, p)),
        TypeExpr::Unit => false,
    }
}

/// The pattern matching values of a type (`Int`, `List[Int]`, `Address`).
fn type_pattern(t: &TypeExpr) -> Option<Pattern> {
    match t {
        TypeExpr::Named(n, args) if args.is_empty() => Some(Pattern::Ctor(*n, CtorPat::None)),
        TypeExpr::Named(n, args) => Some(Pattern::Ctor(*n, CtorPat::Type(args.iter().map(type_pattern).collect::<Option<_>>()?))),
        // a refined member: its base (a union's members are told apart by
        // their JSON, so the base is enough to know which it is)
        TypeExpr::Refined(b, _) => type_pattern(b),
        _ => None,
    }
}

/// A value as a key (a map of the values made): the code that writes it.
fn key(v: &Expr) -> String {
    crate::printer::expr(v, 0)
}

impl<'a> Expander<'a> {
    fn new(env: &'a Env, prog: &'a Program) -> Self {
        Expander {
            env,
            prog,
            known: HashMap::new(), types: HashMap::new(), written: RefCell::default(), self_ty: TypeExpr::Unit, copies: vec![], made: HashMap::new(), ctors: RefCell::default(), hidden: vec![], arm_fields: vec![], params: vec![], trait_name: Symbol::new(""), trait_methods: vec![], defaults: RefCell::default() }
    }

    /// `derive Trait for Type { written }` as an ordinary `impl`.
    fn derive(&mut self, i: &ImplDecl) -> Res<ImplDecl> {
        let short = |n: &str| crate::names::short(n).to_string();
        let tr = self.prog.decls.iter().find_map(|d| match &d.kind {
            DeclKind::Trait(t) if t.name == i.trait_name => Some(t),
            _ => None,
        });
        let Some(tr) = tr else { return Err(format!("`{}` is not a trait", short(&i.trait_name))) };
        let Some(dv) = &tr.derivable else {
            return Err(format!("`{}` can't be derived: only a `derivable` trait can (write `impl {} for {}` instead)", short(&tr.name), short(&tr.name), crate::printer::ty(&i.for_type)));
        };
        // a generic type (`derive Json for Pair`): for `Pair[T]`, each `T` with the trait
        let mut i = i.clone();
        let head0 = crate::types::texpr_head(&i.for_type);
        if let (TypeExpr::Named(n, args), Some(td)) = (&i.for_type, self.env.types.get(&Symbol::new(&head0))) {
            if args.is_empty() && !td.params.is_empty() {
                i.for_type = TypeExpr::Named(*n, td.params.iter().map(|p| TypeExpr::Named(*p, vec![])).collect());
                for p in &td.params {
                    if !i.type_params.contains(p) {
                        i.type_params.push(*p);
                    }
                    if !i.bounds.iter().any(|b| b.param == *p) {
                        i.bounds.push(Bound { param: *p, traits: vec![TypeExpr::Named(tr.name, vec![])] });
                    }
                }
            }
        }
        let i = &i;
        self.params = i.type_params.clone();
        self.self_ty = i.for_type.clone();
        self.trait_name = tr.name;
        self.trait_methods = tr.methods.iter().map(|m| m.decl.name).collect();
        let head = crate::types::texpr_head(&i.for_type);
        match self.env.types.get(&Symbol::new(&head)).map(|td| &td.body) {
            Some(TypeBody::Record(_) | TypeBody::Sum(_)) => {}
            Some(_) if self.env.types.get(&Symbol::new(&head)).is_some_and(crate::types::union_alias) => {}
            Some(TypeBody::Alias(_)) => return Err(format!("`{}` is an alias: a derived trait needs a record or a sum type", short(&head))),
            None => return Err(format!("`{head}` is not a type defined in this program")),
        }
        // the kind: one of `std.typing.Kind` (`Record`), `Choice`, or another
        // refinement of `Type` that the type's value meets; a union of kinds
        // (`Record | Sum`) takes any of them
        let kinds = match &dv.kind {
            TypeExpr::Union(ms) => ms.clone(),
            k => vec![k.clone()],
        };
        let (kind, choice) = self.kind_of(&i.for_type);
        let mut kind_ok = false;
        for k in &kinds {
            let kn = crate::types::texpr_head(k);
            let ks = short(&kn);
            let fits = if kn == q(&ks) && KINDS.contains(&ks.as_str()) {
                kind == ks
            } else if kn == q("Choice") {
                choice
            } else if self.env.types.get(&Symbol::new(&kn)).is_some_and(|td| matches!(&td.body, TypeBody::Alias(TypeExpr::Named(b, _)) if *b == q("Type")) && td.refinement.is_some()) {
                // another refinement of `Type`: the type's value meets it
                let desc = self.type_value(&i.for_type);
                self.of_kind(k, &desc)?
            } else {
                false
            };
            kind_ok = kind_ok || fits;
        }
        let kind_name = kinds.iter().map(|k| format!("`{}`", short(&crate::types::texpr_head(k)))).collect::<Vec<_>>().join(" or a ");
        if !kind_ok {
            return Err(format!("`{}` can only be derived for a {kind_name}, and `{}` isn't one", short(&tr.name), short(&head)));
        }
        for r in &dv.requires {
            // `$Self` (and the like) written in, then run
            let r2 = self.expr(r)?;
            let ok = self.eval_as(&r2, r).map_err(|m| format!("checking what `{}` requires: {m}", short(&tr.name)))?;
            if ok != Expr::Bool(true) {
                return Err(format!("`{}` doesn't meet what `{}` requires: {}", short(&head), short(&tr.name), crate::printer::expr(r, 0)));
            }
        }
        let mut methods = i.methods.clone();
        for m in &tr.methods {
            if i.methods.iter().any(|w| w.name == m.decl.name) {
                continue;
            }
            if m.decl.derived {
                let mut f = m.decl.clone();
                f.derived = false;
                f.body = self.expr(&m.decl.body).map_err(|e| format!("deriving `{}`: {e}", short(&m.decl.name)))?;
                methods.push(f);
            } else if !m.default {
                return Err(format!("`{}` doesn't derive `{}`: write it in `derive {} for {} {{ ... }}`", short(&tr.name), short(&m.decl.name), short(&tr.name), short(&head)));
            }
        }
        Ok(ImplDecl { methods, derived: false, ..i.clone() })
    }

    /// Whether a type's value meets a kind that refines `Type`.
    fn of_kind(&mut self, kind: &TypeExpr, desc: &Expr) -> Res<bool> {
        let TypeExpr::Named(n, _) = kind else { return Err(format!("the kind `{}` must be a type's name", crate::printer::ty(kind))) };
        let x = self.literal(desc)?;
        Ok(self.eval(&Expr::Is(Box::new(x), *n))? == Expr::Bool(true))
    }

    /// A type as a `std.typing.Type` value (remembering which type it is).
    /// A named type's fields and variants are written out, except where
    /// the type is already being written out (a recursive type) or deep
    /// inside another: reading them from a type known while compiling
    /// (`f.type.fields`) writes them out then.
    fn type_value(&self, t: &TypeExpr) -> Expr {
        self.type_value_in(t, &mut vec![])
    }

    fn type_value_in(&self, t: &TypeExpr, stack: &mut Vec<TypeExpr>) -> Expr {
        let short = |n: &Symbol| crate::names::short(n).to_string();
        let mut name = none();
        let mut doc: Vec<String> = vec![];
        let mut args = vec![];
        let (mut fields, mut elements, mut members, mut params, mut result) = (vec![], vec![], vec![], vec![], none());
        let kind = match t {
            // a refinement: its base, with the condition
            TypeExpr::Refined(b, pred) => {
                let mut v = self.type_value_in(b, stack);
                let cond = crate::printer::expr(pred, 0);
                let cond = match str_of(&v, "condition") {
                    Some(c) => format!("{c} && {cond}"),
                    None => cond,
                };
                set(&mut v, "condition", some(Expr::Str(cond)));
                set(&mut v, "written", Expr::Str(crate::printer::ty(t)));
                self.written.borrow_mut().insert(key(&v), t.clone());
                return v;
            }
            TypeExpr::Unit => "Unit",
            TypeExpr::Fn(ps, r) => {
                params = ps.iter().map(|a| self.type_value_in(a, stack)).collect();
                result = some(self.type_value_in(r, stack));
                "Function"
            }
            // a union's members as written (a named union among them is one
            // member, with its own `impl`s), the members a union declares
            // itself (`Square` and `Dot` of `Circle | Square { .. } | Dot`) each
            TypeExpr::Union(ms) => {
                for a in ms {
                    let m = self.type_value_in(a, stack);
                    let own = matches!(a, TypeExpr::Named(n, _) if n.contains(crate::desugar::OWN));
                    match kind_name(&m).as_deref() {
                        Some("Union") if own => match self.full(&m, "members") {
                            Some(Expr::List(xs)) => members.extend(xs),
                            _ => members.push(m),
                        },
                        _ => members.push(m),
                    }
                }
                "Union"
            }
            TypeExpr::Named(n, targs) => {
                name = some(Expr::Str(short(n)));
                args = targs.iter().map(|a| self.type_value_in(a, stack)).collect();
                // written out once on the way down, and not too deep
                let shallow = stack.contains(t) || stack.len() >= 4;
                stack.push(t.clone());
                let kind = if n.as_str() == "Tuple" {
                    elements = args.clone();
                    "Tuple"
                } else if let Some(td) = self.env.types.get(n) {
                    doc = self.prog.decls.iter().find(|d| matches!(&d.kind, DeclKind::Type(x) if x.name == *n)).map(|d| d.doc.clone()).unwrap_or_default();
                    let sub: crate::fx::HashMap<Symbol, TypeExpr> = td.params.iter().cloned().zip(targs.iter().cloned()).collect();
                    let s = |x: &TypeExpr| crate::types::subst_texpr(x, &sub);
                    match &td.body {
                        TypeBody::Record(fs) => {
                            if !shallow {
                                fields = fs.iter().map(|f| self.field(&f.name, &f.doc, &s(&f.ty), f.default.as_ref(), stack)).collect();
                            }
                            "Record"
                        }
                        // a sum: a union of the members it declares
                        TypeBody::Sum(vs) => {
                            if !shallow {
                                members = vs.iter().map(|v| self.variant_value(v, targs, &sub, stack)).collect();
                            }
                            "Union"
                        }
                        // an alias or a `distinct` type: the shape of what it
                        // stands for, under its own name
                        TypeBody::Alias(a) => {
                            let full = match &td.refinement {
                                Some(r) => TypeExpr::Refined(Box::new(s(a)), Box::new(r.clone())),
                                None => s(a),
                            };
                            let mut v = self.type_value_in(&full, stack);
                            stack.pop();
                            set(&mut v, "name", some(Expr::Str(short(n))));
                            set(&mut v, "doc", strs(&doc));
                            set(&mut v, "args", Expr::List(args));
                            set(&mut v, "written", Expr::Str(crate::printer::ty(t)));
                            self.written.borrow_mut().insert(key(&v), t.clone());
                            return v;
                        }
                    }
                } else if let Some(v) = self.variant_decl(n) {
                    // a variant used as a type (`TooBig`)
                    stack.pop();
                    let sum = self.env.ctors[n];
                    let sub: crate::fx::HashMap<Symbol, TypeExpr> = self.env.types[&sum].params.iter().cloned().zip(targs.iter().cloned()).collect();
                    return self.variant_value(&v, targs, &sub, stack);
                } else if matches!(n.as_str(), "Int" | "Float" | "Bool" | "String" | "List" | "Set" | "Dict") {
                    "Builtin"
                } else {
                    "Param"
                };
                stack.pop();
                kind
            }
        };
        let v = record(
            "Type",
            vec![
                ("name", name),
                ("doc", strs(&doc)),
                ("kind", atom(&q(kind))),
                ("args", Expr::List(args)),
                ("written", Expr::Str(crate::printer::ty(t))),
                ("condition", none()),
                ("fields", Expr::List(fields)),
                ("elements", Expr::List(elements)),
                ("members", Expr::List(members)),
                ("params", Expr::List(params)),
                ("result", result),
            ],
        );
        self.written.borrow_mut().insert(key(&v), t.clone());
        v
    }

    /// The declaration of variant `n`, if it is one.
    fn variant_decl(&self, n: &Symbol) -> Option<Variant> {
        let sum = self.env.ctors.get(n)?;
        match &self.env.types.get(sum)?.body {
            TypeBody::Sum(vs) => vs.iter().find(|v| v.name == *n).cloned(),
            _ => None,
        }
    }

    /// A sum's variant as a `Type`: a record, a tuple or an atom.
    fn variant_value(&self, v: &Variant, targs: &[TypeExpr], sub: &crate::fx::HashMap<Symbol, TypeExpr>, stack: &mut Vec<TypeExpr>) -> Expr {
        let s = |x: &TypeExpr| crate::types::subst_texpr(x, sub);
        let (kind, fields, elements) = match &v.payload {
            Payload::Type(_) => unreachable!("a union member defined elsewhere is desugared away"),
            Payload::None => ("Atom", vec![], vec![]),
            Payload::Positional(ts) => ("Tuple", vec![], ts.iter().map(|t| self.type_value_in(&s(t), stack)).collect()),
            Payload::Record(fs) => ("Record", fs.iter().map(|f| self.field(&f.name, &f.doc, &s(&f.ty), f.default.as_ref(), stack)).collect(), vec![]),
        };
        let name = crate::names::short(&v.name).to_string();
        let d = record(
            "Type",
            vec![
                ("name", some(Expr::Str(name.clone()))),
                ("doc", Expr::List(vec![])),
                ("kind", atom(&q(kind))),
                ("args", Expr::List(vec![])),
                ("written", Expr::Str(name)),
                ("condition", none()),
                ("fields", Expr::List(fields)),
                ("elements", Expr::List(elements)),
                ("members", Expr::List(vec![])),
                ("params", Expr::List(vec![])),
                ("result", none()),
            ],
        );
        let key = key(&d);
        self.ctors.borrow_mut().insert(key.clone(), v.name.to_string());
        self.written.borrow_mut().insert(key, TypeExpr::Named(v.name, targs.to_vec()));
        d
    }

    fn field(&self, name: &Symbol, doc: &[String], ty: &TypeExpr, default: Option<&Expr>, stack: &mut Vec<TypeExpr>) -> Expr {
        // a refinement written as a type argument, as written (not its `$where_` alias)
        let ty = &self.unwhere(ty);
        let t = self.type_value_in(ty, stack);
        let dv = match default {
            Some(e) => some(Expr::Str(crate::printer::expr(e, 0))),
            None => none(),
        };
        let f = record("Field", vec![("name", Expr::Str(name.to_string())), ("doc", strs(doc)), ("type", t), ("default", dv)]);
        if let Some(e) = default {
            self.defaults.borrow_mut().insert(key(&f), e.clone());
        }
        f
    }

    /// `t` with each alias made for a refinement written as a type argument
    /// (`$where_0`) back as that refinement (`Float where 0.0 <= it`).
    fn unwhere(&self, t: &TypeExpr) -> TypeExpr {
        match t {
            TypeExpr::Named(n, args) if args.is_empty() && n.as_str().starts_with(crate::desugar::WHERE) => match self.env.types.get(n) {
                Some(TypeDecl { body: TypeBody::Alias(b), refinement: Some(r), .. }) => TypeExpr::Refined(Box::new(b.clone()), Box::new(r.clone())),
                _ => t.clone(),
            },
            TypeExpr::Named(n, args) => TypeExpr::Named(*n, args.iter().map(|a| self.unwhere(a)).collect()),
            TypeExpr::Refined(b, p) => TypeExpr::Refined(Box::new(self.unwhere(b)), p.clone()),
            TypeExpr::Fn(ps, r) => TypeExpr::Fn(ps.iter().map(|a| self.unwhere(a)).collect(), Box::new(self.unwhere(r))),
            TypeExpr::Union(ms) => TypeExpr::Union(ms.iter().map(|a| self.unwhere(a)).collect()),
            TypeExpr::Unit => TypeExpr::Unit,
        }
    }

    /// The type of a variant, if `t` is one: `TooBig` of `ShapeError`.
    fn is_variant(&self, v: &Expr) -> bool {
        self.ctors.borrow().contains_key(&key(v))
    }

    /// The type of a `Type` value.
    fn type_of(&self, v: &Expr) -> Res<TypeExpr> {
        self.written.borrow().get(&key(v)).cloned().ok_or(format!("{} is not a type known here", key(v)))
    }

    /// `t.fields` (or another list) of a `Type` value, written out in full.
    fn full(&self, v: &Expr, key: &str) -> Option<Expr> {
        let t = self.type_of(v).ok()?;
        get(&self.type_value(&t), key).cloned()
    }

    /// The kind of a type (`"Record"`, `"Sum"`, ..: a `std.typing.Kind`),
    /// aliases seen through and refinements set aside; and whether it is a
    /// sum of atoms (a `Choice`).
    fn kind_of(&self, t: &TypeExpr) -> (&'static str, bool) {
        match t {
            TypeExpr::Refined(b, _) => self.kind_of(b),
            TypeExpr::Unit => ("Unit", false),
            TypeExpr::Fn(..) => ("Function", false),
            // a choice: its members (as `members` lists them) all atoms
            TypeExpr::Union(ms) => ("Union", ms.iter().all(|m| match m {
                TypeExpr::Named(n, _) if n.contains(crate::desugar::OWN) => self.kind_of(m).1,
                _ => self.kind_of(m).0 == "Atom",
            })),
            TypeExpr::Named(n, _) if n.as_str() == "Tuple" => ("Tuple", false),
            TypeExpr::Named(n, _) => match self.env.types.get(n).map(|td| &td.body) {
                Some(TypeBody::Record(_)) => ("Record", false),
                Some(TypeBody::Sum(vs)) => ("Union", vs.iter().all(|v| match &v.payload {
                    Payload::None => true,
                    Payload::Type(t) => self.kind_of(t).0 == "Atom",
                    _ => false,
                })),
                Some(TypeBody::Alias(a)) => self.kind_of(a),
                None => match self.variant_decl(n).map(|v| v.payload) {
                    Some(Payload::Type(_)) => unreachable!("a union member defined elsewhere is desugared away"),
                    Some(Payload::None) => ("Atom", false),
                    Some(Payload::Positional(_)) => ("Tuple", false),
                    Some(Payload::Record(_)) => ("Record", false),
                    None if matches!(n.as_str(), "Int" | "Float" | "Bool" | "String" | "List" | "Set" | "Dict") => ("Builtin", false),
                    None => ("Param", false),
                },
            },
        }
    }

    /// The type a `distinct` type is made of: its name, if `t` is one.
    fn distinct_head(&self, t: &TypeExpr) -> Option<Symbol> {
        let TypeExpr::Named(n, _) = t else { return None };
        self.env.types.get(n).filter(|td| td.distinct).map(|_| *n)
    }

    /// The type an expression names while compiling: a type parameter bound
    /// by `match comptime` or a `comptime t: Type` (`t`), or a field's type (`f.type`).
    fn type_named(&self, e: &Expr) -> Option<TypeExpr> {
        match e {
            _ if is_self(e) => Some(self.self_ty.clone()),
            Expr::Var(x) if !self.hidden.iter().any(|h| h.contains(x)) => self.types.get(x.as_str()).cloned(),
            Expr::Field(r, d) if d == "type" => match &**r {
                Expr::Var(x) if self.is_known(x) => self.types.get(&format!("{x}.type")).cloned(),
                _ => None,
            },
            _ => None,
        }
    }

    fn is_known(&self, x: &Symbol) -> bool {
        self.known.contains_key(x) && !self.hidden.iter().any(|h| h.contains(x))
    }

    /// Run compile-time code, with the known values in scope.
    fn eval(&mut self, e: &Expr) -> Res<Expr> {
        self.eval_as(e, e)
    }

    /// `eval`, messages showing `written` (the code before the known values
    /// were written in).
    fn eval_as(&mut self, e: &Expr, written: &Expr) -> Res<Expr> {
        // most compile-time code is data: a description written out, a known
        // name, a field of one; nothing runs
        if let Some(v) = self.data(e) {
            return Ok(v);
        }
        self.compiled(e, written)
    }

    /// Run compile-time code compiled: a function whose body is `e`, the
    /// known values bound before it, checked with the program as it is
    /// before its derived `impl`s (compile-time code can't use them), run
    /// within the budget, its value read back.
    fn compiled(&mut self, e: &Expr, written: &Expr) -> Res<Expr> {
        let shown = || crate::printer::expr(written, 0);
        let name = Symbol::new("strata.comptime");
        let mut stmts = vec![];
        let mut known: Vec<(&Symbol, &Expr)> = self.known.iter().filter(|(k, _)| self.is_known(k)).collect();
        known.sort_by_key(|(k, _)| k.as_str());
        for (k, v) in known {
            if let Ok(x) = self.literal(v) {
                stmts.push(Stmt::Let(Pattern::Bind(*k), None, x));
            }
        }
        stmts.push(Stmt::Let(Pattern::Bind(Symbol::new("strata_ct")), None, self.with_self(e)?));
        let f = FnDecl {
            doc: vec![],
            comments: vec![],
            name,
            type_params: vec![],
            bounds: vec![],
            params: vec![],
            ret: TypeExpr::Unit,
            requires: vec![],
            ensures: vec![],
            terminates: false,
            body: Expr::Block(stmts, Box::new(Expr::Unit)),
            comptime: false,
            derived: false,
        };
        let mut p = Program { decls: vec![] };
        for d in &crate::types::rooted(self.prog).decls {
            match &d.kind {
                DeclKind::Impl(i) if i.derived => {}
                DeclKind::Test(_) => {}
                // ordinary code with its own `comptime` still to evaluate (the
                // one being evaluated, perhaps): resolving it would evaluate that
                DeclKind::Fn(f) if !is_template(f) && any_expr(&f.body, |x| matches!(x, Expr::Comptime(_))) => {}
                _ => p.decls.push(d.clone()),
            }
        }
        p.decls.extend(self.copies.iter().map(|c| Decl { comments: vec![], doc: vec![], kind: DeclKind::Fn(c.clone()), module: Symbol::new(""), public: false }));
        p.decls.push(Decl { comments: vec![], doc: vec![], kind: DeclKind::Fn(f), module: Symbol::new(""), public: false });
        // pure and deterministic: its value depends only on this function and what it reaches
        let key = cache_key(&p, name.as_str());
        if let Some(v) = key.as_ref().and_then(|k| self.cached(k)) {
            return Ok(v);
        }
        let rep = crate::types::check_report_resolved(&p);
        if let Some(err) = rep.errors.iter().find(|d| d.decl == name.as_str()) {
            return Err(format!("`comptime {}`: {}", shown(), err.msg));
        }
        // its type, as the checker found it, is the function's result: `{ known; e }`
        let (r, types) = crate::types::elaborate_resolved(&p);
        let ty = r.program.decls.iter().find_map(|d| match &d.kind {
            DeclKind::Fn(f) if f.name == name => match &f.body {
                Expr::Block(stmts, _) => match stmts.last() {
                    Some(Stmt::Let(_, _, x)) => types.get(&(x as *const Expr as usize)).cloned(),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        });
        let ty = ty.ok_or(format!("`comptime {}`: its type isn't known", shown()))?;
        let Some(Decl { kind: DeclKind::Fn(f), .. }) = p.decls.last_mut() else { unreachable!() };
        let Expr::Block(stmts, _) = std::mem::replace(&mut f.body, Expr::Unit) else { unreachable!() };
        let mut stmts = stmts;
        let Some(Stmt::Let(_, _, x)) = stmts.pop() else { unreachable!() };
        f.body = Expr::Block(stmts, Box::new(x));
        f.ret = crate::types::ty_to_texpr(&ty);
        let (ir, roots, shapes) = crate::llvm::untraced(|| crate::llvm::compile_comptime(&p, &[name.to_string()])).map_err(|m| format!("`comptime {}` can't be compiled: {m}", shown()))?;
        let mut vs = crate::llvm::jit::comptime(&ir, &roots, &shapes, BUDGET as i64)?;
        match vs.pop() {
            Some(Ok(v)) => {
                if let (Some(k), Ok(code)) = (&key, self.literal(&v)) {
                    let _ = std::fs::create_dir_all(cache_dir()).and_then(|_| std::fs::write(cache_dir().join(k), crate::printer::expr(&code, 0)));
                }
                Ok(v)
            }
            Some(Err(f)) if f.message == "too many steps" => Err(format!("`comptime {}` ran for more than {BUDGET} steps: compile-time code must finish", shown())),
            Some(Err(f)) => Err(format!("`comptime {}` failed: {}", shown(), f.message)),
            None => Err(format!("`comptime {}` gave nothing", shown())),
        }
    }

    /// `e` with `Self` (the type derived for, as a value) written in.
    fn with_self(&self, e: &Expr) -> Res<Expr> {
        struct W(Option<Expr>);
        impl VisitMut for W {
            fn expr(&mut self, e: &mut Expr) {
                match (&*e, &self.0) {
                    (x, Some(v)) if is_self(x) => *e = v.clone(),
                    _ => walk_expr_mut(self, e),
                }
            }
        }
        let v = if self.self_ty == TypeExpr::Unit { None } else { Some(self.literal(&self.type_value(&self.self_ty))?) };
        let mut out = e.clone();
        W(v).expr(&mut out);
        Ok(out)
    }

    /// A compile-time value computed before (`cache_key`): its code, read as data.
    fn cached(&self, key: &str) -> Option<Expr> {
        let text = std::fs::read_to_string(cache_dir().join(key)).ok()?;
        let e = crate::parser::parse_expr(&text).ok()?;
        self.data(&e)
    }

    /// The value of `e` if it is data: literals, constructors of them, known
    /// names and their fields (as the interpreter would make it). `None`
    /// when something would have to run.
    fn data(&self, e: &Expr) -> Option<Expr> {
        self.data_in(e, &mut vec![])
    }

    /// `data`, with a lambda's parameters bound (`locals`, innermost last).
    fn data_in(&self, e: &Expr, locals: &mut Vec<(Symbol, Expr)>) -> Option<Expr> {
        Some(match e {
            Expr::Var(x) if locals.iter().any(|(n, _)| n == x) => locals.iter().rev().find(|(n, _)| n == x)?.1.clone(),
            // `None is Some`: a constructor's name, known (a type's, as `Choice`, may be refined)
            Expr::Is(x, c) if self.env.ctors.contains_key(c) && !self.env.types.contains_key(c) => match self.data_in(x, locals)? {
                Expr::Ctor(n, _) => Expr::Bool(crate::names::short(&n) == crate::names::short(c)),
                _ => return None,
            },
            // `$Self.fields.map(f -> f.name)`: each item through a lambda that is data too
            Expr::Method(recv, m, args) if m == "map" && args.len() == 1 => {
                let Expr::List(items) = self.data_in(recv, locals)? else { return None };
                let Expr::Lambda(ps, body) = &args[0] else { return None };
                let [p] = ps.as_slice() else { return None };
                let mut out = vec![];
                for it in items {
                    locals.push((p.name, it));
                    let v = self.data_in(body, locals);
                    locals.pop();
                    out.push(v?);
                }
                Expr::List(out)
            }
            Expr::Int(n) => Expr::Int(*n),
            Expr::BigInt(b) => crate::llvm::native::int_expr(b.parse().ok()?),
            Expr::Float(x) => Expr::Float(*x),
            Expr::Bool(b) => Expr::Bool(*b),
            Expr::Str(s) => Expr::Str(s.clone()),
            Expr::Unit => Expr::Unit,
            Expr::Var(x) if self.is_known(x) => self.known[x].clone(),
            // `Self`: the type derived for
            _ if is_self(e) && self.self_ty != TypeExpr::Unit => self.type_value(&self.self_ty),
            // `a ?? b`
            Expr::Or(a, b) => match self.data_in(a, locals)? {
                Expr::Ctor(n, CtorArgs::Positional(xs)) if n == "Some" => xs.first()?.clone(),
                Expr::Ctor(n, CtorArgs::None) if n == "None" => self.data_in(b, locals)?,
                _ => return None,
            },
            // comparing what is known (`if comptime f.name == "type"`): nothing to run
            Expr::Bin(op, a, b) => {
                use BinOp::*;
                let (x, y) = (self.data_in(a, locals)?, self.data_in(b, locals)?);
                match (op, &x, &y) {
                    (Eq, _, _) => Expr::Bool(x == y),
                    (Ne, _, _) => Expr::Bool(x != y),
                    (And, Expr::Bool(p), Expr::Bool(q)) => Expr::Bool(*p && *q),
                    (Or, Expr::Bool(p), Expr::Bool(q)) => Expr::Bool(*p || *q),
                    (Lt | Le | Gt | Ge, Expr::Int(p), Expr::Int(q)) => Expr::Bool(match op {
                        Lt => p < q,
                        Le => p <= q,
                        Gt => p > q,
                        _ => p >= q,
                    }),
                    _ => return None,
                }
            }
            Expr::Un(UnOp::Not, x) => match self.data_in(x, locals)? {
                Expr::Bool(b) => Expr::Bool(!b),
                _ => return None,
            },
            Expr::Call(f, args) if matches!(&**f, Expr::Var(n) if n == "len") && args.len() == 1 => match self.data_in(&args[0], locals)? {
                Expr::List(xs) => Expr::Int(xs.len() as i64),
                _ => return None,
            },
            Expr::List(xs) => Expr::List(xs.iter().map(|x| self.data_in(x, locals)).collect::<Option<_>>()?),
            Expr::Tuple(xs) => Expr::Tuple(xs.iter().map(|x| self.data_in(x, locals)).collect::<Option<_>>()?),
            Expr::Field(r, f) => match self.data_in(r, locals)? {
                // a type's lists, written out in full (`f.type.fields`)
                v @ Expr::Ctor(..) if matches!(&v, Expr::Ctor(k, _) if *k == q("Type")) && matches!(f.as_str(), "fields" | "elements" | "members") => self.full(&v, f.as_str())?,
                Expr::Ctor(_, CtorArgs::Record(fs)) => fs.into_iter().find(|(k, _)| k == f.as_str())?.1,
                _ => return None,
            },
            Expr::Ctor(c, args) => {
                let tname = *self.env.ctors.get(c)?;
                let td = self.env.types.get(&tname)?;
                if td.distinct || td.refinement.is_some() {
                    return None;
                }
                match args {
                    CtorArgs::None => Expr::Ctor(*c, CtorArgs::None),
                    CtorArgs::Positional(es) => Expr::Ctor(*c, CtorArgs::Positional(es.iter().map(|x| self.data_in(x, locals)).collect::<Option<_>>()?)),
                    CtorArgs::Record(fs) => {
                        // in declaration order, every field given (a default would have to run)
                        let order = self.env.variants_of(&tname, &[]).and_then(|vs| vs.into_iter().find(|v| &v.0 == c)).and_then(|v| v.1)?;
                        if order.len() != fs.len() {
                            return None;
                        }
                        let mut vals = vec![];
                        for f in &order {
                            let (_, fe) = fs.iter().find(|(n, _)| n == f)?;
                            vals.push((*f, self.data_in(fe, locals)?));
                        }
                        Expr::Ctor(*c, CtorArgs::Record(vals))
                    }
                    CtorArgs::Update(..) => return None,
                }
            }
            _ => return None,
        })
    }

    /// A value computed at compile time, written as code: it is already.
    fn literal(&self, v: &Expr) -> Res<Expr> {
        Ok(v.clone())
    }

    /// Specialize code for the known values.
    fn expr(&mut self, e: &Expr) -> Res<Expr> {
        match e {
            Expr::Comptime(x) => {
                // what it uses (`$Self`, known names) written in, then run
                let known = self.expr(x)?;
                let v = self.eval_as(&known, x)?;
                return self.literal(&v);
            }
            // a known value used where the program runs: written in
            Expr::Var(x) if self.is_known(x) => {
                let v = self.known[x].clone();
                return self.literal(&v);
            }
            // `f.default_value`: the field's default, as written (`times: Int = 1`)
            Expr::Field(r, d) if d == "default_value" && matches!(&**r, Expr::Var(x) if self.is_known(x)) => {
                let Expr::Var(x) = &**r else { unreachable!() };
                return match self.defaults.borrow().get(&key(&self.known[x])) {
                    Some(e) => Ok(e.clone()),
                    // only reached where `f.default is Some` holds: never, here
                    None => Ok(Expr::Hole(Symbol::new("no_default"))),
                };
            }
            // `self.0` in the arm of a match comprehension that matched a
            // tuple-shaped variant: its first element
            Expr::Field(r, d) if d.as_str().parse::<usize>().is_ok() && self.arm_fields.iter().any(|(s, _)| *s == crate::printer::expr(r, 0)) => {
                let shown = crate::printer::expr(r, 0);
                let (_, names) = self.arm_fields.iter().rev().find(|(s, _)| *s == shown).unwrap();
                if let Some((_, n)) = names.iter().find(|(k, _)| k == d.as_str()) {
                    return Ok(Expr::Var(*n));
                }
            }
            // `f.name`, `Self.name`, `v.fields`, `v.name ?? ""`: read while compiling
            Expr::Field(..) | Expr::Or(..) if self.data(e).is_some() => {
                let v = self.data(e).unwrap();
                return self.literal(&v);
            }
            // `t.check(x)`: `Ok(())`, or `Err` with the interpreter's message
            Expr::Method(recv, m, args) if m == "check" && self.type_named(recv).is_some() => {
                let t = self.type_named(recv).unwrap();
                let [x] = args.as_slice() else { return Err("`check` takes the value to check".into()) };
                let x = self.expr(x)?;
                return Ok(self.check(&t, &x));
            }
            // `$Self.make(f -> e)`, `v.make()`, `v.try_make(f -> e)`: only on a
            // description known while compiling
            Expr::Method(recv, m, args) if m == "make" || m == "try_make" => {
                let known = self.expr(recv).and_then(|r| self.eval(&r));
                let Ok(desc) = known else {
                    return Err(format!(
                        "`{}.{m}(..)` needs `{}` known at compile time: build each variant in a `comptime for v in $Self.variants` (or a match comprehension)",
                        crate::printer::expr(recv, 0),
                        crate::printer::expr(recv, 0)
                    ));
                };
                return self.make(desc, m == "try_make", args);
            }
            // `f.of(x)`: the field `f` of `x` (a record, or the variant a match
            // comprehension's arm matched)
            Expr::Method(recv, m, args) if m == "of" && matches!(&**recv, Expr::Var(f) if self.is_known(f)) => {
                let Expr::Var(f) = &**recv else { unreachable!() };
                let [x] = args.as_slice() else { return Err("`f.of(x)` takes the value to read the field of".into()) };
                let fname = match &self.known[f] {
                    Expr::Ctor(k, CtorArgs::Record(fs)) if *k == q("Field") => fs.iter().find_map(|(k, v)| match (k.as_str(), v) {
                        ("name", Expr::Str(s)) => Some(s.clone()),
                        _ => None,
                    }),
                    _ => None,
                };
                let Some(fname) = fname else { return Err(format!("`{f}.of(..)` reads a field: `{f}` must be a field's description")) };
                let x = self.expr(x)?;
                let shown = crate::printer::expr(&x, 0);
                if let Some((_, names)) = self.arm_fields.iter().rev().find(|(s, _)| *s == shown) {
                    if let Some(n) = names.iter().find(|(k, _)| *k == fname) {
                        return Ok(Expr::Var(n.1));
                    }
                }
                if fname.parse::<usize>().is_ok() {
                    return Err(format!("`{f}.of({shown})`: a positional field is read in the arm of a match comprehension that matched its variant"));
                }
                return Ok(Expr::Field(Box::new(x), Symbol::new(&fname)));
            }
            // `t.decode(j)`, `f.type.decode(j)`: a method of a type known while
            // compiling, called on that type (`Address.decode(j)`)
            Expr::Method(recv, m, args) if !matches!(m.as_str(), "make" | "try_make" | "check" | "of") && self.type_named(recv).is_some() => {
                let t = self.type_named(recv).unwrap();
                // (a named union is called by its name: its `impl`s are its own)
                let named = match &t {
                    TypeExpr::Named(n, a) if self.env.types.get(n).is_some_and(crate::types::union_alias) => Some(TypeExpr::Named(*n, a.clone())),
                    _ => None,
                };
                let TypeExpr::Named(n, targs) = named.unwrap_or_else(|| self.base(&t)) else {
                    return Err(format!("`{}.{m}(..)`: `{}` has no methods of its own to call", crate::printer::expr(recv, 0), crate::printer::ty(&t)));
                };
                // (a generic type, `Pair[Int]`: called on `Pair`, its arguments are
                // inferred from what the call must return)
                let _ = targs;
                // a method of the trait being derived: the type must have it (said
                // here, at the `derive`, rather than in the code made for it)
                let own = crate::types::texpr_head(&self.self_ty) == n.as_str() || self.params.contains(&n);
                if !own && self.trait_methods.contains(m) && !self.env.impls.iter().any(|i| i.trait_name == self.trait_name && crate::types::texpr_head(&i.for_type) == n.as_str()) {
                    let tn = crate::names::short(&self.trait_name);
                    return Err(format!("`{}` doesn't implement `{tn}`: write `derive {tn} for {}`", crate::names::short(&n), crate::names::short(&n)));
                }
                let args = args.iter().map(|a| self.expr(a)).collect::<Res<Vec<_>>>()?;
                return Ok(Expr::Method(Box::new(Expr::Var(n)), *m, args));
            }
            // `if comptime c { a } else { b }`: only the branch taken is kept (so a
            // `compile_error` in the other isn't reached)
            Expr::If(c, a, b) if any_expr(c, |x| matches!(x, Expr::Comptime(_))) => {
                let c = self.expr(c)?;
                return match self.eval(&c) {
                    Ok(Expr::Bool(true)) => self.expr(a),
                    Ok(Expr::Bool(false)) => self.expr(b),
                    _ => Err(format!("`if {}`: the condition must be known while compiling", crate::printer::expr(&c, 0))),
                };
            }
            Expr::Block(stmts, last) => return self.block(stmts, last),
            Expr::Match(s, name, arms) if arms.iter().any(|a| matches!(a.pat, Pattern::For(..))) => return self.comprehension(s, *name, arms),
            Expr::Match(s, None, arms) if matches!(&**s, Expr::Comptime(_)) => {
                let Expr::Comptime(x) = &**s else { unreachable!() };
                return self.match_type(x, arms);
            }
            // `compile_error(msg)`: reached in specialized code, the `derive` fails with it
            Expr::Call(f, args) if matches!(&**f, Expr::Var(n) if crate::names::short(n) == "compile_error") && args.len() == 1 => {
                let m = self.expr(&args[0])?;
                return Err(match self.eval(&m)? {
                    Expr::Str(s) => s,
                    other => key(&other),
                });
            }
            // `setting("json.unknown_keys")`: the package's setting (`strata.toml`), if set
            Expr::Call(f, args) if matches!(&**f, Expr::Var(n) if n.as_str() == "std.typing.setting") && args.len() == 1 => {
                let k = self.expr(&args[0])?;
                let Expr::Str(key) = self.eval(&k)? else { return Err("`setting` takes the setting's name, `section.key`".into()) };
                return Ok(match crate::package::setting(&key) {
                    Some(v) => Expr::Ctor(Symbol::new("Some"), CtorArgs::Positional(vec![Expr::Str(v)])),
                    None => Expr::Ctor(Symbol::new("None"), CtorArgs::None),
                });
            }
            // `t(v)`: a value of the `distinct` type `t`, known while compiling
            Expr::Call(f, args) if self.type_named(f).is_some_and(|t| self.distinct_head(&t).is_some()) => {
                let head = self.distinct_head(&self.type_named(f).unwrap()).unwrap();
                let args = args.iter().map(|a| self.expr(a)).collect::<Res<Vec<_>>>()?;
                return Ok(Expr::Ctor(head, CtorArgs::Positional(args)));
            }
            Expr::Call(f, args) => {
                if let Expr::Var(name) = &**f {
                    if let Some(fd) = self.env.fns.get(name).filter(|fd| fd.params.iter().any(|p| p.comptime)).cloned() {
                        return self.call_copy(name, &fd, args);
                    }
                }
            }
            Expr::Lambda(ps, _) => {
                self.hidden.push(ps.iter().map(|p| p.name).collect());
                let r = self.children(e);
                self.hidden.pop();
                return r;
            }
            _ => {}
        }
        self.children(e)
    }

    /// `e` with its children specialized.
    fn children(&mut self, e: &Expr) -> Res<Expr> {
        struct Walk<'x, 'a> {
            x: &'x mut Expander<'a>,
            err: Option<String>,
            top: bool,
        }
        impl VisitMut for Walk<'_, '_> {
            fn expr(&mut self, e: &mut Expr) {
                if self.err.is_some() {
                    return;
                }
                if std::mem::replace(&mut self.top, false) {
                    return walk_expr_mut(self, e);
                }
                match self.x.expr(e) {
                    Ok(n) => *e = n,
                    Err(m) => self.err = Some(m),
                }
            }
            fn ty(&mut self, t: &mut TypeExpr) {
                *t = self.x.ty(t);
            }
        }
        let mut out = e.clone();
        let mut w = Walk { x: self, err: None, top: true };
        w.expr(&mut out);
        match w.err {
            Some(m) => Err(m),
            None => Ok(out),
        }
    }

    /// A type with the known ones written in (`f.type`, an arm's `x`, `Self`).
    fn ty(&self, t: &TypeExpr) -> TypeExpr {
        match t {
            TypeExpr::Named(n, args) if args.is_empty() && self.types.contains_key(n.as_str()) => self.types[n.as_str()].clone(),
            TypeExpr::Named(n, args) if args.is_empty() && n == "Self" => self.self_ty.clone(),
            TypeExpr::Named(n, args) => TypeExpr::Named(*n, args.iter().map(|a| self.ty(a)).collect()),
            TypeExpr::Fn(ps, r) => TypeExpr::Fn(ps.iter().map(|a| self.ty(a)).collect(), Box::new(self.ty(r))),
            TypeExpr::Refined(b, p) => TypeExpr::Refined(Box::new(self.ty(b)), p.clone()),
            TypeExpr::Union(ms) => TypeExpr::Union(ms.iter().map(|a| self.ty(a)).collect()),
            TypeExpr::Unit => TypeExpr::Unit,
        }
    }

    /// `t.make(f -> e)`: the type's constructor, with `e` for each field
    /// (`f` known as that field); `t.make((i, e) -> x)` for a tuple, with
    /// `x` for each element (`i` its index, `e` its type); `v.make()` an
    /// atom. `try_make`: each value is a `Result`, the first `Err` is the
    /// result, else `Ok` of the value.
    fn make(&mut self, d: Expr, fallible: bool, args: &[Expr]) -> Res<Expr> {
        let Some(kind) = kind_name(&d) else { return Err("`make` is called on a type known while compiling".into()) };
        let t = self.type_of(&d)?;
        let variant = self.is_variant(&d);
        let list = |k: &str| match self.full(&d, k) {
            Some(Expr::List(xs)) => xs,
            _ => vec![],
        };
        let (fields, elements) = (list("fields"), list("elements"));
        // a record is built by its type's name; a variant by its own
        let head = if variant {
            Some(self.ctors.borrow()[&key(&d)].clone())
        } else if kind == "Record" {
            Some(crate::types::texpr_head(&t).to_string())
        } else if kind == "Tuple" {
            None
        } else {
            return Err(format!("`make` builds a record, a tuple or a variant, not a `{}`", crate::printer::ty(&t)));
        };
        let mut vals: Vec<(Symbol, Expr)> = vec![];
        let each = |this: &mut Self, binds: Vec<(Symbol, Expr, Option<(String, TypeExpr)>)>, body: &Expr| -> Res<Expr> {
            let mut saved = vec![];
            for (n, v, ty) in binds {
                saved.push((n, this.known.insert(n, v), ty.as_ref().map(|(k, t)| (k.clone(), this.types.insert(k.clone(), t.clone())))));
            }
            let e = this.expr(body);
            for (n, old, ty) in saved.into_iter().rev() {
                match old {
                    Some(v) => this.known.insert(n, v),
                    None => this.known.remove(&n),
                };
                if let Some((k, old)) = ty {
                    match old {
                        Some(t) => this.types.insert(k, t),
                        None => this.types.remove(&k),
                    };
                }
            }
            e
        };
        match (kind.as_str(), args) {
            ("Atom", []) => {}
            ("Record", []) | ("Tuple", []) if fields.is_empty() && elements.is_empty() => {}
            ("Record", [Expr::Lambda(ps, body)]) if ps.len() == 1 => {
                let p = ps[0].name;
                for f in &fields {
                    let fname = str_of(f, "name").unwrap_or_default();
                    let ft = get(f, "type").and_then(|v| self.type_of(v).ok());
                    let e = each(self, vec![(p, f.clone(), ft.map(|t| (format!("{p}.type"), t)))], body)?;
                    vals.push((Symbol::new(&fname), e));
                }
            }
            ("Tuple", [Expr::Lambda(ps, body)]) if ps.len() == 2 => {
                let (pi, pe) = (ps[0].name, ps[1].name);
                for (k, el) in elements.iter().enumerate() {
                    let et = self.type_of(el).ok();
                    let e = each(self, vec![(pi, Expr::Int(k as i64), None), (pe, el.clone(), et.map(|t| (pe.to_string(), t)))], body)?;
                    vals.push((Symbol::new(&k.to_string()), e));
                }
            }
            ("Record", _) => return Err("`make` builds a record from a function giving each field's value: `make(f -> e)`".into()),
            ("Tuple", _) => return Err("`make` builds a tuple from a function giving each element's value, from its index and type: `make((i, e) -> x)`".into()),
            _ => return Err("`make()` builds an atom: it takes nothing".into()),
        }
        let positional = kind == "Tuple";
        let ctor = |vals: Vec<(Symbol, Expr)>| match &head {
            None => Expr::Tuple(vals.into_iter().map(|(_, e)| e).collect()),
            Some(h) => {
                let args = if kind == "Atom" {
                    CtorArgs::None
                } else if positional {
                    CtorArgs::Positional(vals.into_iter().map(|(_, e)| e).collect())
                } else {
                    CtorArgs::Record(vals)
                };
                Expr::Ctor(Symbol::new(h), args)
            }
        };
        if !fallible {
            return Ok(ctor(vals));
        }
        // match e1 { Ok(v1) => match e2 { ... Ok(T { .. }) }, Err(e) => Err(e) }
        let names: Vec<Symbol> = (0..vals.len()).map(|k| Symbol::new(&format!("made.{k}"))).collect();
        let mut out = Expr::Ctor(Symbol::new("Ok"), CtorArgs::Positional(vec![ctor(vals.iter().zip(&names).map(|((f, _), n)| (*f, Expr::Var(*n))).collect())]));
        for ((_, e), n) in vals.into_iter().zip(&names).rev() {
            let err = Symbol::new("made.err");
            out = Expr::Match(
                Box::new(e),
                None,
                vec![
                    Arm { comments: vec![], pat: Pattern::Ctor(Symbol::new("Ok"), CtorPat::Positional(vec![Pattern::Bind(*n)])), body: out },
                    Arm { comments: vec![], pat: Pattern::Ctor(Symbol::new("Err"), CtorPat::Positional(vec![Pattern::Bind(err)])), body: Expr::Ctor(Symbol::new("Err"), CtorArgs::Positional(vec![Expr::Var(err)])) },
                ],
            );
        }
        Ok(out)
    }

    /// A block, with each `comptime for` written out: its body once per
    /// element of the list (known while compiling), the element known in each.
    fn block(&mut self, stmts: &[Stmt], last: &Expr) -> Res<Expr> {
        let mut out = vec![];
        for st in stmts {
            if let Stmt::ComptimeFor(p, xs, body) = st {
                let Pattern::Bind(x) = p else { return Err("`comptime for` names its element: `comptime for v in ..`".into()) };
                let list = self.expr(xs)?;
                let Expr::List(items) = self.eval(&list).map_err(|e| format!("the list of a `comptime for` must be known at compile time: {e}"))? else {
                    return Err("`comptime for` goes over a list".into());
                };
                for it in items {
                    let saved = self.known.insert(*x, it);
                    let b = self.expr(body);
                    match saved {
                        Some(v) => self.known.insert(*x, v),
                        None => self.known.remove(x),
                    };
                    out.push(Stmt::Expr(b?));
                }
                continue;
            }
            // any other statement: specialized as a one-statement block
            match self.children(&Expr::Block(vec![st.clone()], Box::new(Expr::Unit)))? {
                Expr::Block(mut s1, _) => out.append(&mut s1),
                _ => unreachable!(),
            }
        }
        let last = self.expr(last)?;
        Ok(Expr::Block(out, Box::new(last)))
    }

    /// A match whose arms come from a comprehension (`v => e for v in $Self.variants`):
    /// one arm per element, written out. A variant's description as the
    /// arm's pattern matches that variant (whatever its fields).
    fn comprehension(&mut self, scrut: &Expr, name: Option<Symbol>, arms: &[Arm]) -> Res<Expr> {
        let s = self.expr(scrut)?;
        let mut out = vec![];
        for arm in arms {
            let Pattern::For(p, x, xs) = &arm.pat else {
                let m = self.children(&Expr::Match(Box::new(Expr::Unit), None, vec![arm.clone()]))?;
                if let Expr::Match(_, _, mut a) = m {
                    out.append(&mut a);
                }
                continue;
            };
            let Pattern::Bind(x) = &**x else { return Err("a match comprehension names its element: `.. for v in ..`".into()) };
            let list = self.expr(xs)?;
            let Expr::List(items) = self.eval(&list).map_err(|e| format!("the list of a match comprehension must be known at compile time: {e}"))? else {
                return Err("a match comprehension goes over a list".into());
            };
            let shown = crate::printer::expr(&s, 0);
            for it in items {
                let saved = self.known.insert(*x, it.clone());
                let pat = self.arm_pattern(p, x, &it);
                let names = match &pat {
                    Ok(Pattern::Ctor(_, CtorPat::Positional(ps))) => ps.iter().enumerate().filter_map(|(k, p)| if let Pattern::Bind(n) = p { Some((k.to_string(), *n)) } else { None }).collect(),
                    Ok(Pattern::Ctor(_, CtorPat::Record(fs, _))) => fs.iter().filter_map(|(k, p)| if let Pattern::Bind(n) = p { Some((k.to_string(), *n)) } else { None }).collect(),
                    _ => vec![],
                };
                self.arm_fields.push((shown.clone(), names));
                let body = self.expr(&arm.body);
                self.arm_fields.pop();
                match saved {
                    Some(v) => self.known.insert(*x, v),
                    None => self.known.remove(x),
                };
                out.push(Arm { comments: vec![], pat: pat?, body: body? });
            }
        }
        Ok(Expr::Match(Box::new(s), name, out))
    }

    /// An arm's pattern with the element written in: the element's own name
    /// (a variant's description) matches that variant.
    fn arm_pattern(&self, p: &Pattern, x: &Symbol, it: &Expr) -> Res<Pattern> {
        match p {
            Pattern::Bind(y) if y == x => match it {
                // a variant: matched by its constructor, its fields or elements
                // bound (`of.r`, `of.0`) for `f.of(x)` and `x.0` in the arm
                _ if self.is_variant(it) => {
                    let c = self.ctors.borrow()[&key(it)].clone();
                    let names = |k: &str| match self.full(it, k) {
                        Some(Expr::List(xs)) => xs,
                        _ => vec![],
                    };
                    let cp = match kind_name(it).as_deref() {
                        Some("Record") => CtorPat::Record(names("fields").iter().map(|f| str_of(f, "name").unwrap_or_default()).map(|n| (Symbol::new(&n), Pattern::Bind(Symbol::new(&format!("of.{n}"))))).collect(), false),
                        Some("Tuple") => CtorPat::Positional((0..names("elements").len()).map(|k| Pattern::Bind(Symbol::new(&format!("of.{k}")))).collect()),
                        _ => CtorPat::None,
                    };
                    Ok(Pattern::Ctor(Symbol::new(&c), cp))
                }
                // a type (a union's member): the type pattern for it
                Expr::Ctor(k, CtorArgs::Record(_)) if *k == q("Type") && self.type_of(it).is_ok() => {
                    // its base type (aliases expanded, refinements set aside): the
                    // members' JSON can't overlap, so the base tells them apart
                    let t = self.base(&self.type_of(it)?);
                    type_pattern(&t).ok_or_else(|| format!("a union member `{}` can't be told apart at run time: use a named type", crate::printer::ty(&t)))
                }
                other => match self.literal(other)? {
                    Expr::Int(n) => Ok(Pattern::Int(n)),
                    Expr::Str(s) => Ok(Pattern::Str(s)),
                    Expr::Bool(b) => Ok(Pattern::Bool(b)),
                    _ => Err("a match comprehension's element must be a variant, a number, a string or a Bool to be a pattern".into()),
                },
            },
            other => Ok(other.clone()),
        }
    }

    /// Code checking `x` against `t`, as `Interp::check_type` does: the
    /// base type's refinement first, then the type's own.
    fn check(&self, t: &TypeExpr, x: &Expr) -> Expr {
        let ok = || Expr::Ctor(Symbol::new("Ok"), CtorArgs::Positional(vec![Expr::Unit]));
        // `if { it := x; cond } { Ok(()) } else { Err(msg) }`, after `first`
        let test = |cond: &Expr, msg: String, first: Expr| {
            let holds = Expr::Block(vec![Stmt::Let(Pattern::Bind(Symbol::new("it")), None, x.clone())], Box::new(cond.clone()));
            let shown = Expr::Call(Box::new(Expr::Var(Symbol::new("to_json"))), vec![x.clone()]);
            let err = Expr::Ctor(Symbol::new("Err"), CtorArgs::Positional(vec![Expr::Interp(vec![StrPart::Expr(shown, None), StrPart::Lit(format!(" {msg}"))])]));
            let this = Expr::If(Box::new(holds), Box::new(ok()), Box::new(err));
            let e = Symbol::new("checked.err");
            Expr::Match(
                Box::new(first),
                None,
                vec![
                    Arm { comments: vec![], pat: Pattern::Ctor(Symbol::new("Ok"), CtorPat::Positional(vec![Pattern::Wild])), body: this },
                    Arm { comments: vec![], pat: Pattern::Ctor(Symbol::new("Err"), CtorPat::Positional(vec![Pattern::Bind(e)])), body: Expr::Ctor(Symbol::new("Err"), CtorArgs::Positional(vec![Expr::Var(e)])) },
                ],
            )
        };
        match t {
            TypeExpr::Refined(b, pred) => test(pred, format!("does not satisfy `{}`", crate::printer::ty(t)), self.check(b, x)),
            TypeExpr::Named(n, args) if args.is_empty() => match self.env.types.get(n) {
                Some(td) if !td.distinct && matches!(td.body, TypeBody::Alias(_)) => {
                    let TypeBody::Alias(a) = &td.body else { unreachable!() };
                    let base = self.check(a, x);
                    match &td.refinement {
                        Some(r) => test(r, format!("does not satisfy `{}` (where {})", crate::names::short(n), crate::printer::expr(r, 0)), base),
                        None => base,
                    }
                }
                _ => ok(),
            },
            _ => ok(),
        }
    }

    /// `match comptime ty { Int => .., Option[x] => .., _ => .. }`: the arm
    /// whose type pattern fits, with its names bound to types.
    fn match_type(&mut self, scrut: &Expr, arms: &[Arm]) -> Res<Expr> {
        let t = match self.type_named(scrut) {
            Some(t) => t,
            None => {
                let v = self.eval(scrut)?;
                self.type_of(&v)?
            }
        };
        for arm in arms {
            let mut binds = vec![];
            if self.fits(&arm.pat, &t, &mut binds) {
                let saved: Vec<_> = binds.iter().map(|(n, ty)| (n.clone(), self.types.insert(n.clone(), ty.clone()))).collect();
                // a bound name also stands for the type's description
                let mut values = vec![];
                for (n, ty) in &binds {
                    let tv = self.type_value(ty);
                    values.push((Symbol::new(n), self.known.insert(Symbol::new(n), tv)));
                }
                let r = self.expr(&arm.body);
                for (n, old) in values {
                    match old {
                        Some(v) => self.known.insert(n, v),
                        None => self.known.remove(&n),
                    };
                }
                for (n, old) in saved {
                    match old {
                        Some(o) => self.types.insert(n, o),
                        None => self.types.remove(&n),
                    };
                }
                return r;
            }
        }
        Err(format!("no arm of `match comptime` fits `{}`", crate::printer::ty(&t)))
    }

    /// Whether a type fits a type pattern (`Int`, `Option[x]`, `_`, `x`).
    /// Aliases are seen through and refinements set aside, so a `Natural`
    /// fits `Int`; a bound name takes the type as written.
    fn fits(&self, p: &Pattern, t: &TypeExpr, binds: &mut Vec<(String, TypeExpr)>) -> bool {
        let base = self.base(t);
        match p {
            Pattern::Wild => true,
            Pattern::Bind(x) => {
                binds.push((x.to_string(), t.clone()));
                true
            }
            // `Choice`: a sum whose variants are all atoms
            Pattern::Ctor(c, CtorPat::None) if c.as_str() == q("Choice") || (c.as_str() == "Choice" && self.env.types.get(c).is_none()) => self.kind_of(t).1,
            // a kind (`std.typing.Kind`): `Record`, `Sum`, `Union`, `Param`, ..
            Pattern::Ctor(c, CtorPat::None) if KINDS.contains(&crate::names::short(c)) && (c.as_str() == q(crate::names::short(c)) || (!c.contains('.') && self.env.types.get(c).is_none())) => {
                let k = self.kind_of(t).0;
                // `Param`: a type parameter of the type derived for (`T` of `Pair[T]`)
                k == crate::names::short(c) && (k != "Param" || matches!(&base, TypeExpr::Named(n, _) if self.params.contains(n)))
            }
            // `Distinct[x]`: a `distinct` type, `x` the type it is made of
            Pattern::Ctor(c, CtorPat::Type(ps)) if crate::names::short(c) == "Distinct" && ps.len() == 1 && self.env.types.get(c).is_none() => {
                let TypeExpr::Named(n, _) = &base else { return false };
                match self.env.types.get(n) {
                    Some(td) if td.distinct => match &td.body {
                        TypeBody::Alias(a) => {
                            let inner = match &td.refinement {
                                Some(r) => TypeExpr::Refined(Box::new(a.clone()), Box::new(r.clone())),
                                None => a.clone(),
                            };
                            self.fits(&ps[0], &inner, binds)
                        }
                        _ => false,
                    },
                    _ => false,
                }
            }
            Pattern::Ctor(c, CtorPat::None) => matches!(&base, TypeExpr::Named(n, _) if crate::names::short(n) == crate::names::short(c)),
            Pattern::Ctor(c, CtorPat::Type(ps)) => match &base {
                TypeExpr::Named(n, args) if crate::names::short(n) == crate::names::short(c) && args.len() == ps.len() => ps.iter().zip(args).all(|(p, a)| self.fits(p, a, binds)),
                _ => false,
            },
            Pattern::Or(ps) => ps.iter().any(|p| self.fits(p, t, binds)),
            _ => false,
        }
    }

    /// A type without its refinements, aliases expanded.
    fn base(&self, t: &TypeExpr) -> TypeExpr {
        match self.env.expand_alias(t) {
            TypeExpr::Refined(b, _) => self.base(&b),
            other => other,
        }
    }

    /// A call of a function with `comptime` parameters: a call of the copy
    /// made for the values given.
    fn call_copy(&mut self, name: &Symbol, fd: &FnDecl, args: &[Expr]) -> Res<Expr> {
        let mut vals = vec![];
        let mut rest = vec![];
        for (p, a) in fd.params.iter().zip(args) {
            if p.comptime {
                // known here: evaluated (once the known values are written in)
                let known = self.expr(a)?;
                vals.push((p.clone(), self.eval_as(&known, a)?));
            } else {
                rest.push(self.expr(a)?);
            }
        }
        let key = format!("{name}({})", vals.iter().map(|(_, v)| key(v)).collect::<Vec<_>>().join(", "));
        if let Some(n) = self.made.get(&key) {
            return Ok(Expr::Call(Box::new(Expr::Var(*n)), rest));
        }
        // named after the type derived for too: each `derive` makes its own copies
        let copy = Symbol::new(&format!("{name}.for.{}.{}", crate::names::short(&crate::types::texpr_head(&self.self_ty)), self.made.len() + 1));
        self.made.insert(key, copy);
        // the copy: the known parameters left out, their values (and types) written
        // in; it sees only those, not what its caller knows
        let outer = (std::mem::take(&mut self.known), std::mem::take(&mut self.types), std::mem::take(&mut self.hidden));
        let mut saved = vec![];
        for (p, v) in &vals {
            saved.push((p.name, self.known.insert(p.name, v.clone())));
            if let Expr::Ctor(_, CtorArgs::Record(fs)) = v {
                if let Some((_, tv)) = fs.iter().find(|(k, _)| k == "type") {
                    if let Ok(t) = self.type_of(tv) {
                        self.types.insert(format!("{}.type", p.name), t);
                    }
                }
            }
            // a `comptime t: Type`: `t` is that type (`$t`, `match comptime t`)
            if let Ok(t) = self.type_of(v) {
                self.types.insert(p.name.to_string(), t);
            }
        }
        let own = self.prog.decls.iter().find_map(|d| match &d.kind {
            DeclKind::Fn(f) if f.name == *name => Some(f.clone()),
            _ => None,
        });
        let own = own.unwrap_or_else(|| fd.clone());
        let body = self.expr(&own.body);
        let params: Vec<Param> = own.params.iter().filter(|p| !p.comptime).map(|p| Param { ty: self.ty(&p.ty), ..p.clone() }).collect();
        let ret = self.ty(&own.ret);
        for (n, old) in saved {
            match old {
                Some(v) => self.known.insert(n, v),
                None => self.known.remove(&n),
            };
            self.types.remove(&format!("{n}.type"));
        }
        let _ = saved;
        (self.known, self.types, self.hidden) = outer;
        // made for a generic type: generic in its parameters that it mentions,
        // each with the trait derived (`fn encode_as.for.Pair.2[T: Json](x: T, ..)`)
        let mut tps = own.type_params.clone();
        let mut bounds = own.bounds.clone();
        for p in self.params.clone() {
            let mentions = params.iter().map(|q| &q.ty).chain(std::iter::once(&ret)).any(|t| mentions_name(t, p));
            if mentions && !tps.contains(&p) {
                tps.push(p);
                bounds.push(Bound { param: p, traits: vec![TypeExpr::Named(self.trait_name, vec![])] });
            }
        }
        let f = FnDecl { name: copy, params, ret, body: body?, comptime: false, type_params: tps, bounds, ..own };
        self.copies.push(f);
        Ok(Expr::Call(Box::new(Expr::Var(copy)), rest))
    }
}
