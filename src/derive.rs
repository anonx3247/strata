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
//! Compile-time code runs in the interpreter, within a step budget.

use crate::ast::*;
use crate::interp::{Fields, Interp, Scope, Value};
use crate::symbol::Symbol;
use crate::types::{Diag, Env};
use std::collections::HashMap;

/// The most steps compile-time code may take, for one `derive`.
const BUDGET: u64 = 5_000_000;

/// Replace each `derive` by its `impl` (or report why it can't be made).
pub fn expand(p: &mut Program, errors: &mut Vec<Diag>) {
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

type Res<X> = Result<X, String>;

struct Expander<'a> {
    env: &'a Env,
    prog: &'a Program,

    it: Interp<'a>,
    /// Values known at compile time, by name (`t`, a `comptime` parameter).
    known: HashMap<Symbol, Value>,
    /// Types known at compile time, by name (`f.type`, `x` in `Option[x] =>`).
    types: HashMap<String, TypeExpr>,
    /// The written type behind each type description made (by its JSON).
    written: HashMap<String, TypeExpr>,
    /// The type derived for.
    self_ty: TypeExpr,
    /// Copies of functions with `comptime` parameters, made for their values.
    copies: Vec<FnDecl>,
    made: HashMap<String, Symbol>,
    /// A variant description's constructor, qualified (by its JSON).
    ctors: HashMap<String, String>,
    /// Names bound inside the code being specialized, which hide known ones.
    hidden: Vec<Vec<Symbol>>,
}

fn q(name: &str) -> String {
    format!("std.typing.{name}")
}

fn record(name: &str, fields: Vec<(&str, Value)>) -> Value {
    Value::Record(q(name), fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn strs(xs: &[String]) -> Value {
    Value::List(xs.iter().map(|s| Value::Str(s.clone())).collect())
}

fn json(v: &Value) -> String {
    crate::interp::to_json(v).to_string()
}

impl<'a> Expander<'a> {
    fn new(env: &'a Env, prog: &'a Program) -> Self {
        let mut it = Interp::new(env, 1);
        it.budget = Some(BUDGET);
        Expander { env, prog, it, known: HashMap::new(), types: HashMap::new(), written: HashMap::new(), self_ty: TypeExpr::Unit, copies: vec![], made: HashMap::new(), ctors: HashMap::new(), hidden: vec![] }
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
        self.self_ty = i.for_type.clone();
        let head = crate::types::texpr_head(&i.for_type);
        let desc = self.describe(&head)?;
        // the kind: the description is a value of it (a refinement, as `Choice` is, holds too)
        let kind_ok = match (&desc, self.base_record(&dv.kind)) {
            (Value::Record(n, _), Some(want)) => *n == want,
            _ => false,
        };
        let kind_name = short(&crate::types::texpr_head(&dv.kind));
        if !kind_ok || self.it.check_type(&dv.kind, &desc).map_err(|f| f.msg)?.is_some() {
            return Err(format!("`{}` can only be derived for a `{kind_name}`, and `{}` isn't one", short(&tr.name), short(&head)));
        }
        let _ = desc;
        for r in &dv.requires {
            // `$Self` (and the like) written in, then run
            let r2 = self.expr(r)?;
            let ok = self.it.eval(&r2, &None).map_err(|f| format!("checking what `{}` requires: {}", short(&tr.name), f.msg))?;
            if ok != Value::Bool(true) {
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

    /// The record type a kind is (or refines): `Record`, `Sum` (`Choice` is a `Sum`).
    fn base_record(&self, kind: &TypeExpr) -> Option<String> {
        let n = crate::types::texpr_head(kind);
        let td = self.env.types.get(&Symbol::new(&n))?;
        match &td.body {
            TypeBody::Record(_) => Some(n.to_string()),
            TypeBody::Alias(a) => self.base_record(a),
            TypeBody::Sum(_) => None,
        }
    }

    /// A type's description: a `std.typing.Record` or `Sum`.
    fn describe(&mut self, head: &str) -> Res<Value> {
        let td = self.env.types.get(&Symbol::new(head)).ok_or(format!("`{head}` is not a type defined in this program"))?.clone();
        let doc = self.prog.decls.iter().find(|d| matches!(&d.kind, DeclKind::Type(t) if t.name == head)).map(|d| d.doc.clone()).unwrap_or_default();
        let name = Value::Str(crate::names::short(head).to_string());
        Ok(match &td.body {
            TypeBody::Record(fs) => {
                let fields = fs.iter().map(|f| self.field(&f.name, &f.doc, &f.ty, f.default.as_ref())).collect();
                record("Record", vec![("name", name), ("doc", strs(&doc)), ("fields", Value::List(fields))])
            }
            TypeBody::Sum(vs) => {
                let variants = vs
                    .iter()
                    .map(|v| {
                        let fields = match &v.payload {
                            Payload::None => vec![],
                            Payload::Positional(ts) => ts.iter().enumerate().map(|(k, t)| self.field(&Symbol::new(&k.to_string()), &[], t, None)).collect(),
                            Payload::Record(fs) => fs.iter().map(|f| self.field(&f.name, &f.doc, &f.ty, f.default.as_ref())).collect(),
                        };
                        let d = record("Variant", vec![("name", Value::Str(crate::names::short(&v.name).to_string())), ("doc", Value::List(vec![])), ("fields", Value::List(fields))]);
                        self.ctors.insert(json(&d), v.name.to_string());
                        d
                    })
                    .collect();
                record("Sum", vec![("name", name), ("doc", strs(&doc)), ("variants", Value::List(variants))])
            }
            TypeBody::Alias(_) => return Err(format!("`{}` is an alias: a derived trait needs a record or a sum type", crate::names::short(head))),
        })
    }

    fn field(&mut self, name: &Symbol, doc: &[String], ty: &TypeExpr, default: Option<&Expr>) -> Value {
        let t = self.type_value(ty);
        let default = match default {
            Some(e) => Value::Variant("Some".into(), Fields::Pos(vec![Value::Str(crate::printer::expr(e, 0))])),
            None => Value::Variant("None".into(), Fields::None),
        };
        record("Field", vec![("name", Value::Str(name.to_string())), ("doc", strs(doc)), ("type", t), ("written", Value::Str(crate::printer::ty(ty))), ("default", default)])
    }

    /// A written type as a `std.typing.Type` (remembering which it was).
    fn type_value(&mut self, t: &TypeExpr) -> Value {
        let v = match t {
            TypeExpr::Named(n, args) => {
                let args = args.iter().map(|a| self.type_value(a)).collect();
                Value::Variant(q("Named"), Fields::Rec(vec![("name".into(), Value::Str(crate::names::short(n).to_string())), ("args".into(), Value::List(args))]))
            }
            TypeExpr::Refined(b, pred) => {
                let base = self.type_value(b);
                Value::Variant(q("Refined"), Fields::Rec(vec![("base".into(), base), ("condition".into(), Value::Str(crate::printer::expr(pred, 0)))]))
            }
            TypeExpr::Fn(ps, r) => {
                let ps = ps.iter().map(|a| self.type_value(a)).collect();
                let r = self.type_value(r);
                Value::Variant(q("Function"), Fields::Rec(vec![("params".into(), Value::List(ps)), ("result".into(), r)]))
            }
            TypeExpr::Unit => Value::Variant(q("Nothing"), Fields::None),
            TypeExpr::Union(ms) => {
                let ms = ms.iter().map(|a| self.type_value(a)).collect();
                Value::Variant(q("Union"), Fields::Rec(vec![("members".into(), Value::List(ms))]))
            }
        };
        self.written.insert(json(&v), t.clone());
        v
    }

    /// The written type a description stands for.
    fn type_of(&self, v: &Value) -> Res<TypeExpr> {
        self.written.get(&json(v)).cloned().ok_or(format!("{} is not a type known here", json(v)))
    }

    fn is_known(&self, x: &Symbol) -> bool {
        self.known.contains_key(x) && !self.hidden.iter().any(|h| h.contains(x))
    }

    /// Run compile-time code, with the known values in scope.
    fn eval(&mut self, e: &Expr) -> Res<Value> {
        let mut scope: Scope = None;
        for (k, v) in &self.known {
            scope = crate::interp::extend(&scope, k, v.clone());
        }
        let r = self.it.eval(e, &scope);
        if self.it.exhausted() {
            return Err(format!("`comptime {}` ran for more than {BUDGET} steps: compile-time code must finish", crate::printer::expr(e, 0)));
        }
        r.map_err(|f| format!("`comptime {}` failed: {}", crate::printer::expr(e, 0), f.msg))
    }

    /// A value computed at compile time, written as code.
    fn literal(&self, v: &Value) -> Res<Expr> {
        Ok(match v {
            Value::Int(n) => Expr::Int(*n),
            Value::Big(b) => Expr::BigInt(b.to_string()),
            Value::Float(x) => Expr::Float(*x),
            Value::Bool(b) => Expr::Bool(*b),
            Value::Str(s) => Expr::Str(s.clone()),
            Value::Unit => Expr::Unit,
            Value::List(xs) => Expr::List(xs.iter().map(|x| self.literal(x)).collect::<Res<_>>()?),
            Value::Tuple(xs) => Expr::Tuple(xs.iter().map(|x| self.literal(x)).collect::<Res<_>>()?),
            Value::Record(n, fs) => Expr::Ctor(Symbol::new(n), CtorArgs::Record(fs.iter().map(|(k, x)| Ok((Symbol::new(k), self.literal(x)?))).collect::<Res<_>>()?)),
            Value::Variant(n, Fields::None) => Expr::Ctor(Symbol::new(n), CtorArgs::None),
            Value::Variant(n, Fields::Pos(xs)) => Expr::Ctor(Symbol::new(n), CtorArgs::Positional(xs.iter().map(|x| self.literal(x)).collect::<Res<_>>()?)),
            Value::Variant(n, Fields::Rec(fs)) => Expr::Ctor(Symbol::new(n), CtorArgs::Record(fs.iter().map(|(k, x)| Ok((Symbol::new(k), self.literal(x)?))).collect::<Res<_>>()?)),
            Value::Distinct(n, x) => Expr::Ctor(Symbol::new(n), CtorArgs::Positional(vec![self.literal(x)?])),
            Value::Dict(kvs) => Expr::Call(Box::new(Expr::Var(Symbol::new("dict"))), vec![Expr::List(kvs.iter().map(|(k, x)| Ok(Expr::Tuple(vec![self.literal(k)?, self.literal(x)?]))).collect::<Res<_>>()?)]),
            Value::Set(xs) => Expr::Call(Box::new(Expr::Var(Symbol::new("set"))), vec![Expr::List(xs.iter().map(|x| self.literal(x)).collect::<Res<_>>()?)]),
            Value::Closure(..) | Value::FnRef(_) | Value::Mock(_) => return Err("a function made at compile time can't be kept in the program".into()),
        })
    }

    /// Specialize code for the known values.
    fn expr(&mut self, e: &Expr) -> Res<Expr> {
        match e {
            Expr::Comptime(x) => {
                // what it uses (`$Self`, known names) written in, then run
                let x = self.expr(x)?;
                let v = self.eval(&x)?;
                return self.literal(&v);
            }
            // a known value used where the program runs: written in
            Expr::Var(x) if self.is_known(x) => {
                let v = self.known[x].clone();
                return self.literal(&v);
            }
            // `$Self`, `$f.type`: a type's description
            // `f.default_value`: the field's default, as written (`times: Int = 1`)
            Expr::Field(r, d) if d == "default_value" && matches!(&**r, Expr::Var(x) if self.is_known(x)) => {
                let Expr::Var(x) = &**r else { unreachable!() };
                let name = match &self.known[x] {
                    Value::Record(_, fs) => fs.iter().find_map(|(k, v)| match (k.as_str(), v) {
                        ("name", Value::Str(s)) => Some(s.clone()),
                        _ => None,
                    }),
                    _ => None,
                }
                .unwrap_or_default();
                let head = crate::types::texpr_head(&self.self_ty);
                let fields = match self.env.types.get(&Symbol::new(&head)).map(|t| &t.body) {
                    Some(TypeBody::Record(fs)) => fs.clone(),
                    _ => vec![],
                };
                return match fields.iter().find(|f| f.name == name).and_then(|f| f.default.clone()) {
                    Some(e) => Ok(e),
                    // only reached where `f.default is Some` holds: never, here
                    None => Ok(Expr::Hole(Symbol::new("no_default"))),
                };
            }
            // `$T.check(x)`: `Ok(())`, or `Err` with the interpreter's message
            Expr::Method(recv, m, args) if m == "check" && matches!(&**recv, Expr::Describe(_)) => {
                let Expr::Describe(t) = &**recv else { unreachable!() };
                let [x] = args.as_slice() else { return Err("`check` takes the value to check".into()) };
                let t = self.ty(t);
                let x = self.expr(x)?;
                return Ok(self.check(&t, &x));
            }
            Expr::Describe(t) => {
                let t = self.ty(t);
                let d = self.describe(&crate::types::texpr_head(&t))?;
                return self.literal(&d);
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
                    Value::Str(s) => s,
                    other => json(&other),
                });
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
    /// (`f` known as that field); `try_make`: each `e` is a `Result`, the
    /// first `Err` is the result, else `Ok` of the value.
    fn make(&mut self, d: Value, fallible: bool, args: &[Expr]) -> Res<Expr> {
        let Value::Record(kind, desc) = d else { return Err("`make` is called on a type's description".into()) };
        let Some((_, Value::List(fields))) = desc.iter().find(|(k, _)| k == "fields") else { return Err("`make` builds a record (`$Self.make(f -> e)`) or a variant (`v.make(..)`)".into()) };
        // a variant without fields: `v.make()`
        let none = (Symbol::new("_"), Expr::Unit);
        let (p, body) = match args {
            [Expr::Lambda(ps, body)] if ps.len() == 1 => (ps[0].name, (**body).clone()),
            [] if fields.is_empty() => (none.0, none.1),
            [] => return Err("`make()` without a function only builds what has no fields: pass `f -> e` to give each field's value".into()),
            _ => return Err("`make` takes one function, `f -> e`".into()),
        };
        let p = LamParam { name: p, ty: None };
        let p = &p;
        let body = &body;
        // a record is built by its type's name; a variant by its own, in the sum's module
        let head = crate::types::texpr_head(&self.self_ty);
        let variant = kind == q("Variant");
        let head = if variant {
            let vname = desc.iter().find_map(|(k, v)| match (k.as_str(), v) {
                ("name", Value::Str(s)) => Some(s.clone()),
                _ => None,
            }).unwrap_or_default();
            self.ctors.get(&json(&Value::Record(kind.clone(), desc.clone()))).cloned().unwrap_or_else(|| crate::names::qualify(crate::names::module_of(&head), &vname))
        } else {
            head.to_string()
        };
        let positional = variant && fields.first().is_some_and(|f| matches!(f, Value::Record(_, fd) if fd.iter().any(|(k, v)| k == "name" && *v == Value::Str("0".into()))));
        let mut vals = vec![];
        for fv in fields {
            let Value::Record(_, fd) = fv else { continue };
            let fname = fd.iter().find(|(k, _)| k == "name").and_then(|(_, v)| if let Value::Str(s) = v { Some(s.clone()) } else { None }).unwrap_or_default();
            let ftype = fd.iter().find(|(k, _)| k == "type").map(|(_, v)| v.clone()).unwrap_or(Value::Unit);
            let saved = self.known.insert(p.name, fv.clone());
            let saved_ty = self.type_of(&ftype).ok().map(|t| self.types.insert(format!("{}.type", p.name), t));
            let e = self.expr(body);
            self.types.remove(&format!("{}.type", p.name));
            if let Some(Some(old)) = saved_ty {
                self.types.insert(format!("{}.type", p.name), old);
            }
            match saved {
                Some(v) => self.known.insert(p.name, v),
                None => self.known.remove(&p.name),
            };
            vals.push((Symbol::new(&fname), e?));
        }
        let ctor = |vals: Vec<(Symbol, Expr)>| {
            let args = if variant && vals.is_empty() {
                CtorArgs::None
            } else if positional {
                CtorArgs::Positional(vals.into_iter().map(|(_, e)| e).collect())
            } else {
                CtorArgs::Record(vals)
            };
            Expr::Ctor(Symbol::new(&head), args)
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
                let Value::List(items) = self.eval(&list).map_err(|e| format!("the list of a `comptime for` must be known at compile time: {e}"))? else {
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
            let Value::List(items) = self.eval(&list).map_err(|e| format!("the list of a match comprehension must be known at compile time: {e}"))? else {
                return Err("a match comprehension goes over a list".into());
            };
            for it in items {
                let saved = self.known.insert(*x, it.clone());
                let pat = self.arm_pattern(p, x, &it);
                let body = self.expr(&arm.body);
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
    fn arm_pattern(&self, p: &Pattern, x: &Symbol, it: &Value) -> Res<Pattern> {
        match p {
            Pattern::Bind(y) if y == x => match it {
                Value::Record(k, d) if *k == q("Variant") => {
                    let vname = d.iter().find_map(|(k, v)| match (k.as_str(), v) {
                        ("name", Value::Str(s)) => Some(s.clone()),
                        _ => None,
                    }).unwrap_or_default();
                    let head = crate::types::texpr_head(&self.self_ty);
                    // the variant's shape: nothing, `Rect(_, _)`, `Circle { .. }`
                    let fields = match d.iter().find(|(k, _)| k == "fields") {
                        Some((_, Value::List(fs))) => fs.clone(),
                        _ => vec![],
                    };
                    let positional = fields.first().is_some_and(|f| matches!(f, Value::Record(_, fd) if fd.iter().any(|(k, v)| k == "name" && *v == Value::Str("0".into()))));
                    let cp = if fields.is_empty() {
                        CtorPat::None
                    } else if positional {
                        CtorPat::Positional(vec![Pattern::Wild; fields.len()])
                    } else {
                        CtorPat::Record(vec![], true)
                    };
                    let c = self.ctors.get(&json(it)).cloned().unwrap_or_else(|| crate::names::qualify(crate::names::module_of(&head), &vname));
                    Ok(Pattern::Ctor(Symbol::new(&c), cp))
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
        let v = self.eval(scrut)?;
        let t = self.type_of(&v)?;
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
            // a kind: `Choice` (a sum whose variants carry nothing), `Sum`, `Record`
            Pattern::Ctor(c, CtorPat::None) if matches!(crate::names::short(c), "Choice" | "Sum" | "Record") => {
                let TypeExpr::Named(n, _) = &base else { return false };
                match self.env.types.get(n).map(|t| &t.body) {
                    Some(TypeBody::Sum(vs)) => crate::names::short(c) != "Record" && (crate::names::short(c) == "Sum" || vs.iter().all(|v| v.payload == Payload::None)),
                    Some(TypeBody::Record(_)) => crate::names::short(c) == "Record",
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
                let a = self.expr(a)?;
                vals.push((p.clone(), self.eval(&a)?));
            } else {
                rest.push(self.expr(a)?);
            }
        }
        let key = format!("{name}({})", vals.iter().map(|(_, v)| json(v)).collect::<Vec<_>>().join(", "));
        if let Some(n) = self.made.get(&key) {
            return Ok(Expr::Call(Box::new(Expr::Var(*n)), rest));
        }
        let copy = Symbol::new(&format!("{name}.for.{}", self.made.len() + 1));
        self.made.insert(key, copy);
        // the copy: the known parameters left out, their values (and types) written
        // in; it sees only those, not what its caller knows
        let outer = (std::mem::take(&mut self.known), std::mem::take(&mut self.types), std::mem::take(&mut self.hidden));
        let mut saved = vec![];
        for (p, v) in &vals {
            saved.push((p.name, self.known.insert(p.name, v.clone())));
            if let Value::Record(_, fs) = v {
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
        let f = FnDecl { name: copy, params, ret, body: body?, comptime: false, ..own };
        self.copies.push(f);
        Ok(Expr::Call(Box::new(Expr::Var(copy)), rest))
    }
}
