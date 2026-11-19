//! Tree-walking interpreter with runtime contract checks
//! (refinements, requires, ensures) and an execution trace.

use crate::ast::*;
use crate::generate::Gen;
use crate::types::{subst_texpr, Env};
use serde_json::{json, Value as Json};
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub enum Fields {
    None,
    Pos(Vec<Value>),
    Rec(Vec<(String, Value)>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,
    List(Vec<Value>),
    Tuple(Vec<Value>),
    /// Kept sorted by key (canonical order), so equal dicts are equal values
    /// and serialise identically.
    Dict(Vec<(Value, Value)>),
    /// Kept sorted and duplicate-free.
    Set(Vec<Value>),
    Record(String, Vec<(String, Value)>),
    Variant(String, Fields),
    Closure(Vec<String>, Rc<Expr>, Scope),
    FnRef(String),
    /// A value of a `distinct` type, e.g. `UserId("u_1")`.
    Distinct(String, Box<Value>),
    /// `mock Db`: answers every method with a generated value.
    Mock(TypeExpr),
}

#[derive(Debug, PartialEq)]
pub struct Frame {
    name: String,
    /// A cell, so `let mut` variables can be assigned.
    value: std::cell::RefCell<Value>,
    parent: Scope,
}
pub type Scope = Option<Rc<Frame>>;

impl Frame {
    pub fn new(name: &str, value: Value, parent: Scope) -> Frame {
        Frame { name: name.to_string(), value: std::cell::RefCell::new(value), parent }
    }
}

fn lookup(s: &Scope, x: &str) -> Option<Value> {
    let mut cur = s;
    while let Some(f) = cur {
        if f.name == x {
            return Some(f.value.borrow().clone());
        }
        cur = &f.parent;
    }
    None
}

fn extend(s: &Scope, x: &str, v: Value) -> Scope {
    Some(Rc::new(Frame::new(x, v, s.clone())))
}

/// Set the innermost variable called `x`.
fn assign(s: &Scope, x: &str, v: Value) -> bool {
    let mut cur = s;
    while let Some(f) = cur {
        if f.name == x {
            *f.value.borrow_mut() = v;
            return true;
        }
        cur = &f.parent;
    }
    false
}

/// `base` with the field at `path` replaced (values, not references).
fn set_path(base: Value, path: &[String], v: Value) -> Option<Value> {
    let Some((first, rest)) = path.split_first() else { return Some(v) };
    match base {
        Value::Record(n, mut fs) => {
            let slot = fs.iter_mut().find(|(k, _)| k == first)?;
            slot.1 = set_path(std::mem::replace(&mut slot.1, Value::Unit), rest, v)?;
            Some(Value::Record(n, fs))
        }
        Value::Variant(n, Fields::Rec(mut fs)) => {
            let slot = fs.iter_mut().find(|(k, _)| k == first)?;
            slot.1 = set_path(std::mem::replace(&mut slot.1, Value::Unit), rest, v)?;
            Some(Value::Variant(n, Fields::Rec(fs)))
        }
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum FaultKind {
    /// Execution reached a typed hole.
    Hole,
    /// A refinement type was violated.
    Refinement,
    Requires,
    Ensures,
    Assert,
    /// Arithmetic, missing handler, etc.
    Runtime,
    /// Control flow, caught by the enclosing loop.
    Break,
    Continue,
    /// `return`, `!` or `?`, caught by the enclosing function or lambda; the
    /// value is in `Interp::returning`.
    Return,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Fault {
    pub kind: FaultKind,
    pub msg: String,
    /// Call stack, innermost last.
    pub stack: Vec<String>,
}

type R<T> = Result<T, Fault>;

pub struct Interp<'a> {
    pub env: &'a Env,
    stack: Vec<String>,
    pub trace: Vec<Json>,
    pub tracing: bool,
    pub generator: Gen,
    depth: usize,
    /// compiled regexes for `matches`, which refinements call constantly
    regexes: HashMap<String, regex::Regex>,
    /// Final values of the `mut` parameters of the call that just returned,
    /// to be written back to the caller's variables.
    writeback: Vec<(usize, Value)>,
    /// The value of the `return` (or `!` / `?`) being carried out.
    returning: Option<Value>,
}

fn short(c: &str) -> &str {
    c.rsplit('.').next().unwrap_or(c)
}

fn canon(v: &Value) -> String {
    to_json(v).to_string()
}

pub fn make_dict(pairs: Vec<(Value, Value)>) -> Value {
    // stable sort by key, then keep the last value per key (later entries win, like Python's dict())
    let mut pairs = pairs;
    pairs.sort_by(|a, b| compare(&a.0, &b.0));
    let mut out: Vec<(Value, Value)> = Vec::with_capacity(pairs.len());
    for (k, v) in pairs {
        match out.last_mut() {
            Some(last) if last.0 == k => last.1 = v,
            _ => out.push((k, v)),
        }
    }
    Value::Dict(out)
}

pub fn make_set(items: Vec<Value>) -> Value {
    let mut out = items;
    out.sort_by(compare);
    out.dedup();
    Value::Set(out)
}

pub fn to_json(v: &Value) -> Json {
    match v {
        // distinct types serialise as their underlying value
        Value::Distinct(_, x) => to_json(x),
        Value::Mock(t) => json!(format!("<mock {}>", crate::printer::ty(t))),
        Value::Int(n) => json!(n),
        Value::Float(f) => json!(f),
        Value::Tuple(xs) | Value::Set(xs) => Json::Array(xs.iter().map(to_json).collect()),
        Value::Dict(kvs) if kvs.iter().all(|(k, _)| matches!(k, Value::Str(_))) => Json::Object(
            kvs.iter().map(|(k, v)| (if let Value::Str(s) = k { s.clone() } else { unreachable!() }, to_json(v))).collect(),
        ),
        Value::Dict(kvs) => Json::Array(kvs.iter().map(|(k, v)| json!([to_json(k), to_json(v)])).collect()),
        Value::Bool(b) => json!(b),
        Value::Str(s) => json!(s),
        Value::Unit => Json::Null,
        Value::List(xs) => Json::Array(xs.iter().map(to_json).collect()),
        Value::Record(_, fs) => Json::Object(fs.iter().map(|(k, v)| (k.clone(), to_json(v))).collect()),
        // module paths belong to the code, not the data: `memory.Done` is "Done"
        Value::Variant(c, Fields::None) => json!(short(c)),
        Value::Variant(c, Fields::Pos(xs)) if xs.len() == 1 => json!({ short(c): to_json(&xs[0]) }),
        Value::Variant(c, Fields::Pos(xs)) => json!({ short(c): xs.iter().map(to_json).collect::<Vec<_>>() }),
        Value::Variant(c, Fields::Rec(fs)) => {
            json!({ short(c): Json::Object(fs.iter().map(|(k, v)| (k.clone(), to_json(v))).collect()) })
        }
        Value::Closure(ps, ..) => json!(format!("<closure ({}) -> …>", ps.join(", "))),
        Value::FnRef(n) => json!(format!("<fn {n}>")),
    }
}

pub fn show(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Variant(c, Fields::None) => short(c).to_string(),
        other => to_json(other).to_string(),
    }
}

/// A total order on values, used by `sort`: numbers numerically, strings
/// lexicographically, everything else by canonical JSON.
pub fn compare(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Float(x), Value::Float(y)) => x.total_cmp(y),
        (Value::Str(x), Value::Str(y)) => x.cmp(y),
        (Value::Tuple(xs), Value::Tuple(ys)) | (Value::List(xs), Value::List(ys)) => {
            for (x, y) in xs.iter().zip(ys) {
                let o = compare(x, y);
                if o.is_ne() {
                    return o;
                }
            }
            xs.len().cmp(&ys.len())
        }
        _ => canon(a).cmp(&canon(b)),
    }
}

fn fault(kind: FaultKind, msg: impl Into<String>) -> Fault {
    Fault { kind, msg: msg.into(), stack: vec![] }
}

impl<'a> Interp<'a> {
    pub fn new(env: &'a Env, seed: u64) -> Self {
        Interp {
            env,
            stack: vec![],
            trace: vec![],
            tracing: false,
            generator: Gen::new(seed),
            depth: 0,
            regexes: HashMap::new(),
            writeback: vec![],
            returning: None,
        }
    }

    fn fail<T>(&self, kind: FaultKind, msg: impl Into<String>) -> R<T> {
        Err(Fault { kind, msg: msg.into(), stack: self.stack.clone() })
    }

    // ---------------- contracts ----------------

    /// Evaluate a contract expression.
    fn pure_eval(&mut self, e: &Expr, s: &Scope) -> R<Value> {
        self.eval(e, s)
    }

    /// Check a value against a declared type, including every refinement
    /// reachable through it. Returns a description of the violation.
    pub fn check_type(&mut self, t: &TypeExpr, v: &Value) -> R<Option<String>> {
        self.check_type_in(t, v, &None)
    }

    /// Like `check_type`, with extra bindings visible to inline refinements
    /// (a function's parameters, for its result type).
    pub fn check_type_in(&mut self, t: &TypeExpr, v: &Value, scope: &Scope) -> R<Option<String>> {
        if let TypeExpr::Refined(b, pred) = t {
            if let Some(e) = self.check_type(b, v)? {
                return Ok(Some(e));
            }
            let ok = self.pure_eval(pred, &extend(scope, "it", v.clone()))?;
            if ok != Value::Bool(true) {
                return Ok(Some(format!("{} does not satisfy `{}`", to_json(v), crate::printer::ty(t))));
            }
            return Ok(None);
        }
        if let TypeExpr::Union(ms) = t {
            for m in ms {
                if self.shape_fits(m, v) && self.check_type_in(m, v, scope)?.is_none() {
                    return Ok(None);
                }
            }
            return Ok(Some(format!("{} is not a `{}`", to_json(v), crate::printer::ty(t))));
        }
        let TypeExpr::Named(n, args) = t else { return Ok(None) };
        if let Some(p) = self.env.variant_parent(n) {
            let own = matches!(v, Value::Variant(c, _) if c == n);
            if !own {
                return Ok(Some(format!("{} is not a `{n}`", to_json(v))));
            }
            return self.check_type_in(&TypeExpr::Named(p, args.clone()), v, scope);
        }
        if n == "Tuple" {
            if let Value::Tuple(xs) = v {
                for (i, (t, x)) in args.iter().zip(xs).enumerate() {
                    if let Some(e) = self.check_type(t, x)? {
                        return Ok(Some(format!(".{i}: {e}")));
                    }
                }
            }
            return Ok(None);
        }
        if n == "Set" {
            if let (Value::Set(xs), Some(et)) = (v, args.first()) {
                for x in xs {
                    if let Some(e) = self.check_type(et, x)? {
                        return Ok(Some(format!("element {}: {e}", to_json(x))));
                    }
                }
            }
            return Ok(None);
        }
        if n == "Dict" {
            if let (Value::Dict(kvs), [kt, vt]) = (v, args.as_slice()) {
                for (k, x) in kvs {
                    if let Some(e) = self.check_type(kt, k)? {
                        return Ok(Some(format!("key {}: {e}", to_json(k))));
                    }
                    if let Some(e) = self.check_type(vt, x)? {
                        return Ok(Some(format!("[{}]: {e}", to_json(k))));
                    }
                }
            }
            return Ok(None);
        }
        if n == "List" {
            if let (Value::List(xs), Some(et)) = (v, args.first()) {
                for (i, x) in xs.iter().enumerate() {
                    if let Some(e) = self.check_type(et, x)? {
                        return Ok(Some(format!("[{i}]: {e}")));
                    }
                }
            }
            return Ok(None);
        }
        let Some(td) = self.env.types.get(n).cloned() else { return Ok(None) };
        let sub: HashMap<String, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
        if td.distinct {
            // check the wrapped value against the base type and the refinement
            let Value::Distinct(_, inner) = v else { return Ok(None) };
            if let TypeBody::Alias(a) = &td.body {
                if let Some(e) = self.check_type(&subst_texpr(a, &sub), inner)? {
                    return Ok(Some(e));
                }
            }
            if let Some(r) = &td.refinement {
                if self.pure_eval(r, &extend(&None, "it", (**inner).clone()))? != Value::Bool(true) {
                    return Ok(Some(format!("{} does not satisfy `{n}` (where {})", to_json(inner), crate::printer::expr(r, 0))));
                }
            }
            return Ok(None);
        }
        match &td.body {
            TypeBody::Alias(a) => {
                if let Some(e) = self.check_type(&subst_texpr(a, &sub), v)? {
                    return Ok(Some(e));
                }
            }
            TypeBody::Record(_) | TypeBody::Sum(_) => {
                let variants = self.env.variants_of(n, args).unwrap_or_default();
                let (cname, fvals): (&str, Vec<&Value>) = match v {
                    Value::Record(c, fs) => (c, fs.iter().map(|f| &f.1).collect()),
                    Value::Variant(c, Fields::None) => (c, vec![]),
                    Value::Variant(c, Fields::Pos(xs)) => (c, xs.iter().collect()),
                    Value::Variant(c, Fields::Rec(fs)) => (c, fs.iter().map(|f| &f.1).collect()),
                    _ => return Ok(None),
                };
                if let Some((_, names, ftys)) = variants.iter().find(|x| x.0 == cname) {
                    for (i, (ft, fv)) in ftys.iter().zip(fvals).enumerate() {
                        if let Some(e) = self.check_type(ft, fv)? {
                            let fname = names.as_ref().map(|ns| ns[i].clone()).unwrap_or(i.to_string());
                            return Ok(Some(format!("{cname}.{fname}: {e}")));
                        }
                    }
                }
            }
        }
        if let Some(r) = &td.refinement {
            let ok = self.eval(r, &extend(&None, "it", v.clone()))?;
            if ok != Value::Bool(true) {
                return Ok(Some(format!(
                    "{} does not satisfy `{n}` (where {})",
                    to_json(v),
                    crate::printer::expr(r, 0)
                )));
            }
        }
        Ok(None)
    }

    /// Whether a value is one of the type's values: the right shape, and every
    /// refinement holds. This is what a type pattern and `is` test.
    pub fn value_in(&mut self, t: &TypeExpr, v: &Value) -> R<bool> {
        Ok(self.shape_fits(t, v) && self.check_type(t, v)?.is_none())
    }

    /// Whether a value has the shape of a type, ignoring refinements (which
    /// `check_type` tests). Type parameters accept anything.
    fn shape_fits(&self, t: &TypeExpr, v: &Value) -> bool {
        let t = self.env.expand_alias(t);
        match &t {
            TypeExpr::Unit => matches!(v, Value::Unit),
            TypeExpr::Refined(b, _) => self.shape_fits(b, v),
            TypeExpr::Union(ms) => ms.iter().any(|m| self.shape_fits(m, v)),
            TypeExpr::Fn(..) => matches!(v, Value::Closure(..) | Value::FnRef(_)),
            TypeExpr::Named(n, args) => match (n.as_str(), v) {
                ("Int", Value::Int(_)) | ("Float", Value::Float(_)) | ("Bool", Value::Bool(_)) | ("String", Value::Str(_)) | ("Unit", Value::Unit) => true,
                ("Int" | "Float" | "Bool" | "String" | "Unit", _) => false,
                ("List", Value::List(xs)) | ("Set", Value::Set(xs)) => args.first().is_none_or(|a| xs.iter().all(|x| self.shape_fits(a, x))),
                ("Tuple", Value::Tuple(xs)) => xs.len() == args.len() && args.iter().zip(xs).all(|(a, x)| self.shape_fits(a, x)),
                ("Dict", Value::Dict(_)) => true,
                ("List" | "Set" | "Tuple" | "Dict", _) => false,
                _ if self.env.traits.contains_key(n) => match v {
                    Value::Mock(m) => crate::types::texpr_head(m) == *n,
                    _ => {
                        let head = self.head_of(v);
                        let own = match v { Value::Variant(c, _) => c.clone(), _ => head.clone() };
                        self.env.impls.iter().any(|i| i.trait_name == *n && [&head, &own].contains(&&crate::types::texpr_head(&i.for_type)))
                    }
                },
                _ if self.env.variant_parent(n).is_some() => matches!(v, Value::Variant(c, _) if c == n),
                _ => match self.env.types.get(n) {
                    Some(td) if td.distinct => matches!(v, Value::Distinct(c, _) if c == n),
                    Some(TypeDecl { body: TypeBody::Record(_), .. }) => matches!(v, Value::Record(c, _) if c == n),
                    Some(TypeDecl { body: TypeBody::Sum(vs), .. }) => matches!(v, Value::Variant(c, _) if vs.iter().any(|x| &x.name == c)),
                    // a type parameter, or something unknown
                    _ => true,
                },
            },
        }
    }

    fn enforce(&mut self, t: &TypeExpr, v: &Value, ctx: &str) -> R<()> {
        if let Some(e) = self.check_type(t, v)? {
            return self.fail(FaultKind::Refinement, format!("{ctx}: {e}"));
        }
        Ok(())
    }

    // ---------------- calls ----------------

    pub fn call_fn(&mut self, name: &str, args: Vec<Value>) -> R<Value> {
        let f = self.env.fns.get(name).cloned().ok_or_else(|| fault(FaultKind::Runtime, format!("unknown function `{name}`")))?;
        self.call_decl(&f, name, args)
    }

    /// The runtime type name used to pick a trait implementation.
    pub fn head_of(&self, v: &Value) -> String {
        match v {
            Value::Int(_) => "Int".into(),
            Value::Float(_) => "Float".into(),
            Value::Bool(_) => "Bool".into(),
            Value::Str(_) => "String".into(),
            Value::Unit => "Unit".into(),
            Value::List(_) => "List".into(),
            Value::Tuple(_) => "Tuple".into(),
            Value::Dict(_) => "Dict".into(),
            Value::Set(_) => "Set".into(),
            Value::Record(n, _) => n.clone(),
            Value::Variant(c, _) => self.env.ctors.get(c).cloned().unwrap_or_else(|| c.clone()),
            Value::Closure(..) | Value::FnRef(_) => "Fn".into(),
            Value::Distinct(n, _) => n.clone(),
            Value::Mock(_) => "Mock".into(),
        }
    }

    /// Call a trait method: pick the implementation from the receiver's type.
    /// The trait's own `requires`/`ensures` apply on top of the impl's.
    pub fn call_method(&mut self, name: &str, args: Vec<Value>) -> R<Value> {
        if let Some(Value::Mock(mt)) = args.first() {
            let mt = mt.clone();
            return self.mock_call(&mt, name, args);
        }
        let head = args.first().map(|v| self.head_of(v)).unwrap_or_default();
        // a variant may have an impl of its own; else its sum's applies
        let own = match args.first() {
            Some(Value::Variant(c, _)) => self.env.resolve_method(name, c),
            _ => None,
        };
        let Some((mut decl, req, ens)) = own.or_else(|| self.env.resolve_method(name, &head)) else {
            let tr = self.env.method_trait.get(name).cloned().unwrap_or_default();
            return self.fail(FaultKind::Runtime, format!("`{head}` does not implement `{tr}` (calling `{name}`)"));
        };
        decl.requires.splice(0..0, req);
        decl.ensures.splice(0..0, ens);
        let label = format!("{head}.{name}");
        self.call_decl(&decl, &label, args)
    }

    /// A method call on `mock T`: check the trait's `requires`, then generate a
    /// result that satisfies its `ensures` (rejection sampling). `Self` in the
    /// result is the mock itself.
    fn mock_call(&mut self, mt: &TypeExpr, name: &str, args: Vec<Value>) -> R<Value> {
        let tn = crate::types::texpr_head(mt);
        let Some(tr) = self.env.traits.get(&tn).cloned() else { return self.fail(FaultKind::Runtime, format!("unknown trait `{tn}`")) };
        let Some(tm) = tr.methods.iter().find(|m| m.decl.name == name) else {
            return self.fail(FaultKind::Runtime, format!("`{tn}` has no method `{name}`"));
        };
        let targs = match mt {
            TypeExpr::Named(_, a) if !a.is_empty() => a.clone(),
            _ => tr.params.iter().map(|_| TypeExpr::Named("Int".into(), vec![])).collect(),
        };
        let mut sub: HashMap<String, TypeExpr> = tr.params.iter().cloned().zip(targs).collect();
        sub.insert("Self".into(), mt.clone());
        let mut scope: Scope = None;
        for (p, a) in tm.decl.params.iter().zip(&args) {
            scope = extend(&scope, &p.name, a.clone());
        }
        for r in &tm.decl.requires {
            if self.pure_eval(r, &scope)? != Value::Bool(true) {
                return self.fail(FaultKind::Requires, format!("precondition of `{tn}.{name}` violated: requires {}", crate::printer::expr(r, 0)));
            }
        }
        let ret = subst_texpr(&tm.decl.ret, &sub);
        let env = self.env;
        for _ in 0..200 {
            let mut g = std::mem::replace(&mut self.generator, Gen::new(0));
            let v = g.value(env, &ret, self);
            self.generator = g;
            let v = v.map_err(|e| fault(FaultKind::Runtime, format!("mock {tn}.{name}: {e}")))?;
            let s2 = extend(&scope, "it", v.clone());
            let mut ok = true;
            for en in &tm.decl.ensures {
                if self.pure_eval(en, &s2)? != Value::Bool(true) {
                    ok = false;
                }
            }
            if ok {
                return Ok(v);
            }
        }
        self.fail(FaultKind::Runtime, format!("mock {tn}.{name}: could not generate a result satisfying its `ensures`"))
    }

    pub fn call_decl(&mut self, f: &FnDecl, name: &str, args: Vec<Value>) -> R<Value> {
        let f = f.clone();
        if args.len() != f.params.len() {
            return self.fail(FaultKind::Runtime, format!("`{name}` expects {} arguments", f.params.len()));
        }
        self.depth += 1;
        if self.depth > 2000 {
            return self.fail(FaultKind::Runtime, "stack overflow (recursion depth > 2000)");
        }
        self.stack.push(name.to_string());
        if self.tracing {
            self.trace.push(json!({"event": "call", "fn": name, "args": args.iter().map(to_json).collect::<Vec<_>>(), "depth": self.depth}));
        }
        let mut scope: Scope = None;
        for (p, a) in f.params.iter().zip(args.iter()) {
            self.enforce(&p.ty, a, &format!("argument `{}` of `{name}`", p.name))?;
            scope = extend(&scope, &p.name, a.clone());
        }
        for r in &f.requires {
            if self.eval(r, &scope)? != Value::Bool(true) {
                let shown: Vec<String> = f.params.iter().zip(&args).map(|(p, a)| format!("{} = {}", p.name, to_json(a))).collect();
                return self.fail(
                    FaultKind::Requires,
                    format!("precondition of `{name}` violated: requires {} (with {})", crate::printer::expr(r, 0), shown.join(", ")),
                );
            }
        }
        let result = self.eval_body(&f.body, &scope)?;
        if let Some(e) = self.check_type_in(&f.ret, &result, &scope)? {
            return self.fail(FaultKind::Refinement, format!("result of `{name}`: {e}"));
        }
        if !f.ensures.is_empty() {
            let s2 = extend(&scope, "it", result.clone());
            for en in &f.ensures {
                let ok = self.eval(en, &s2);
                if !matches!(ok, Ok(Value::Bool(true))) {
                    return self.fail(
                        FaultKind::Ensures,
                        format!("postcondition of `{name}` violated: ensures {} (it = {})", crate::printer::expr(en, 0), to_json(&result)),
                    );
                }
            }
        }
        if self.tracing {
            self.trace.push(json!({"event": "return", "fn": name, "value": to_json(&result), "depth": self.depth}));
        }
        self.stack.pop();
        self.depth -= 1;
        self.writeback = f.params.iter().enumerate().filter(|(_, p)| p.mutable).filter_map(|(i, p)| lookup(&scope, &p.name).map(|v| (i, v))).collect();
        Ok(result)
    }

    fn apply(&mut self, f: Value, args: Vec<Value>) -> R<Value> {
        match f {
            Value::FnRef(n) => {
                if self.env.fns.contains_key(&n) {
                    self.call_fn(&n, args)
                } else if self.env.method_trait.contains_key(&n) {
                    self.call_method(&n, args)
                } else {
                    self.builtin(&n, args)
                }
            }
            Value::Closure(ps, body, scope) => {
                if ps.len() != args.len() {
                    return self.fail(FaultKind::Runtime, "closure called with wrong number of arguments");
                }
                let mut s = scope;
                for (p, a) in ps.iter().zip(args) {
                    s = extend(&s, p, a);
                }
                self.eval_body(&body, &s)
            }
            other => self.fail(FaultKind::Runtime, format!("{} is not a function", to_json(&other))),
        }
    }

    fn builtin(&mut self, name: &str, args: Vec<Value>) -> R<Value> {
        use Value::*;
        let mut a = args.into_iter();
        let mut nx = || a.next().unwrap_or(Unit);
        Ok(match name {
            "len" => match nx() {
                List(xs) | Set(xs) => Int(xs.len() as i64),
                Str(s) => Int(s.chars().count() as i64),
                Dict(kvs) => Int(kvs.len() as i64),
                _ => Int(0),
            },
            "dict" => match nx() {
                List(xs) => make_dict(xs.into_iter().filter_map(|p| if let Tuple(mut kv) = p { let v = kv.pop()?; let k = kv.pop()?; Some((k, v)) } else { None }).collect()),
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            "set" => match nx() {
                List(xs) => make_set(xs),
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            "insert" => match (nx(), nx()) {
                (Dict(mut kvs), k) => {
                    let v = nx();
                    kvs.push((k, v));
                    make_dict(kvs)
                }
                (Set(mut xs), x) => {
                    xs.push(x);
                    make_set(xs)
                }
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            "remove" => match (nx(), nx()) {
                (Dict(kvs), k) => Dict(kvs.into_iter().filter(|(k2, _)| *k2 != k).collect()),
                (Set(xs), x) => Set(xs.into_iter().filter(|y| *y != x).collect()),
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            "keys" | "values" | "items" => match nx() {
                Dict(kvs) => List(
                    kvs.into_iter()
                        .map(|(k, v)| match name {
                            "keys" => k,
                            "values" => v,
                            _ => Tuple(vec![k, v]),
                        })
                        .collect(),
                ),
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            "map_values" => {
                let (Dict(kvs), f) = (nx(), nx()) else { return self.fail(FaultKind::Runtime, "bad args") };
                let mut out = Vec::new();
                for (k, v) in kvs {
                    out.push((k, self.apply(f.clone(), vec![v])?));
                }
                Dict(out)
            }
            "to_list" => match nx() {
                Set(xs) => List(xs),
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            "union" | "intersection" | "difference" => {
                let (Set(a), Set(b)) = (nx(), nx()) else { return self.fail(FaultKind::Runtime, "bad args") };
                match name {
                    "union" => make_set([a, b].concat()),
                    "intersection" => Set(a.into_iter().filter(|x| b.contains(x)).collect()),
                    _ => Set(a.into_iter().filter(|x| !b.contains(x)).collect()),
                }
            }
            "zip" => {
                let (List(a), List(b)) = (nx(), nx()) else { return self.fail(FaultKind::Runtime, "bad args") };
                List(a.into_iter().zip(b).map(|(x, y)| Tuple(vec![x, y])).collect())
            }
            "enumerate" => match nx() {
                List(xs) => List(xs.into_iter().enumerate().map(|(i, x)| Tuple(vec![Int(i as i64), x])).collect()),
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            "sort" => match nx() {
                List(mut xs) => {
                    xs.sort_by(compare);
                    List(xs)
                }
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            "sort_by" => {
                let (List(xs), f) = (nx(), nx()) else { return self.fail(FaultKind::Runtime, "bad args") };
                let mut keyed = Vec::new();
                for x in xs {
                    keyed.push((self.apply(f.clone(), vec![x.clone()])?, x));
                }
                keyed.sort_by(|a, b| compare(&a.0, &b.0)); // stable
                List(keyed.into_iter().map(|(_, x)| x).collect())
            }
            "reverse" => match nx() {
                List(mut xs) => {
                    xs.reverse();
                    List(xs)
                }
                Str(s) => Str(s.chars().rev().collect()),
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            "join" => {
                let (List(xs), Str(sep)) = (nx(), nx()) else { return self.fail(FaultKind::Runtime, "bad args") };
                Str(xs.iter().map(show).collect::<Vec<_>>().join(&sep))
            }
            "split" => {
                let (Str(s), Str(sep)) = (nx(), nx()) else { return self.fail(FaultKind::Runtime, "bad args") };
                if sep.is_empty() {
                    return self.fail(FaultKind::Runtime, "split: empty separator");
                }
                List(s.split(sep.as_str()).map(|p| Str(p.to_string())).collect())
            }
            "trim" => match nx() {
                Str(s) => Str(s.trim().to_string()),
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            "parse_int" | "parse_float" => {
                let Str(s) = nx() else { return self.fail(FaultKind::Runtime, "bad args") };
                let parsed = if name == "parse_int" {
                    s.trim().parse::<i64>().ok().map(Int)
                } else {
                    s.trim().parse::<f64>().ok().filter(|f| f.is_finite()).map(Float)
                };
                match parsed {
                    Some(v) => Variant("Some".into(), Fields::Pos(vec![v])),
                    None => Variant("None".into(), Fields::None),
                }
            }
            "float" => match nx() {
                Int(n) => Float(n as f64),
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            "floor" | "ceil" | "round" => match nx() {
                Float(f) => {
                    let r = match name {
                        "floor" => f.floor(),
                        "ceil" => f.ceil(),
                        _ => f.round(),
                    };
                    if !(i64::MIN as f64..=i64::MAX as f64).contains(&r) {
                        return self.fail(FaultKind::Runtime, format!("{f} does not fit in an Int"));
                    }
                    Int(r as i64)
                }
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            "sqrt" => match nx() {
                Float(f) if f >= 0.0 => Float(f.sqrt()),
                Float(f) => return self.fail(FaultKind::Runtime, format!("sqrt of negative number {f}")),
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            "map" | "filter" | "all" | "any" => {
                let (List(xs), f) = (nx(), nx()) else { return self.fail(FaultKind::Runtime, "bad args") };
                let mut out = Vec::new();
                for x in xs {
                    let r = self.apply(f.clone(), vec![x.clone()])?;
                    match name {
                        "map" => out.push(r),
                        "filter" if r == Bool(true) => out.push(x),
                        "all" if r != Bool(true) => return Ok(Bool(false)),
                        "any" if r == Bool(true) => return Ok(Bool(true)),
                        _ => {}
                    }
                }
                match name {
                    "all" => Bool(true),
                    "any" => Bool(false),
                    _ => List(out),
                }
            }
            "fold" => {
                let (List(xs), mut acc, f) = (nx(), nx(), nx()) else { return self.fail(FaultKind::Runtime, "bad args") };
                for x in xs {
                    acc = self.apply(f.clone(), vec![acc, x])?;
                }
                acc
            }
            "contains" => match (nx(), nx()) {
                (List(xs) | Set(xs), x) => Bool(xs.contains(&x)),
                (Dict(kvs), k) => Bool(kvs.iter().any(|(k2, _)| *k2 == k)),
                (Str(s), Str(sub)) => Bool(s.contains(&sub)),
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            "take" | "drop" => {
                let (List(xs), Int(n)) = (nx(), nx()) else { return self.fail(FaultKind::Runtime, "bad args") };
                let n = (n.max(0) as usize).min(xs.len());
                List(if name == "take" { xs[..n].to_vec() } else { xs[n..].to_vec() })
            }
            "find" | "count" => {
                let (List(xs), f) = (nx(), nx()) else { return self.fail(FaultKind::Runtime, "bad args") };
                let mut n = 0;
                for x in xs {
                    if self.apply(f.clone(), vec![x.clone()])? == Bool(true) {
                        if name == "find" {
                            return Ok(Variant("Some".into(), Fields::Pos(vec![x])));
                        }
                        n += 1;
                    }
                }
                if name == "find" { Variant("None".into(), Fields::None) } else { Int(n) }
            }
            "push" => {
                let (List(mut xs), x) = (nx(), nx()) else { return self.fail(FaultKind::Runtime, "bad args") };
                xs.push(x);
                List(xs)
            }
            "get" => {
                let found = match (nx(), nx()) {
                    (List(xs), Int(i)) => (i >= 0).then(|| xs.get(i as usize).cloned()).flatten(),
                    (Str(s), Int(i)) => (i >= 0).then(|| s.chars().nth(i as usize).map(|c| Str(c.to_string()))).flatten(),
                    (Dict(kvs), k) => kvs.into_iter().find(|(k2, _)| *k2 == k).map(|(_, v)| v),
                    _ => return self.fail(FaultKind::Runtime, "bad args"),
                };
                match found.as_ref() {
                    Some(v) => Variant("Some".into(), Fields::Pos(vec![v.clone()])),
                    None => Variant("None".into(), Fields::None),
                }
            }
            "sum" => match nx() {
                List(xs) => {
                    if let Some(Float(_)) = xs.first() {
                        return Ok(Float(xs.iter().map(|x| if let Float(f) = x { *f } else { 0.0 }).sum()));
                    }
                    let mut t: i64 = 0;
                    for x in xs {
                        if let Int(n) = x {
                            t = t.checked_add(n).ok_or_else(|| fault(FaultKind::Runtime, "integer overflow in sum"))?;
                        }
                    }
                    Int(t)
                }
                _ => Int(0),
            },
            "range" => {
                let (Int(lo), Int(hi)) = (nx(), nx()) else { return self.fail(FaultKind::Runtime, "bad args") };
                if hi - lo > 1_000_000 {
                    return self.fail(FaultKind::Runtime, "range too large");
                }
                List((lo..hi).map(Int).collect())
            }
            "abs" => match nx() {
                Int(n) => Int(n.abs()),
                Float(f) => Float(f.abs()),
                _ => Unit,
            },
            "min" | "max" => match (nx(), nx()) {
                (Int(x), Int(y)) => Int(if name == "min" { x.min(y) } else { x.max(y) }),
                (Float(x), Float(y)) => Float(if name == "min" { x.min(y) } else { x.max(y) }),
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            "matches" => {
                let (Str(s), Str(re)) = (nx(), nx()) else { return self.fail(FaultKind::Runtime, "bad args") };
                if !self.regexes.contains_key(&re) {
                    let compiled = regex::Regex::new(&format!("^(?:{re})$")).map_err(|e| fault(FaultKind::Runtime, format!("bad regex: {e}")))?;
                    self.regexes.insert(re.clone(), compiled);
                }
                Bool(self.regexes[&re].is_match(&s))
            }
            "starts_with" => {
                let (Str(s), Str(p)) = (nx(), nx()) else { return self.fail(FaultKind::Runtime, "bad args") };
                Bool(s.starts_with(&p))
            }
            "to_string" => Str(show(&nx())),
            "ends_with" => {
                let (Str(s), Str(p)) = (nx(), nx()) else { return self.fail(FaultKind::Runtime, "bad args") };
                Bool(s.ends_with(&p))
            }
            "chars" => match nx() {
                Str(s) => List(s.chars().map(|c| Str(c.to_string())).collect()),
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            // `slice(x, start, end)`: elements start..end, clamped to the bounds (as in Python)
            "slice" => {
                let (x, Int(a), Int(b)) = (nx(), nx(), nx()) else { return self.fail(FaultKind::Runtime, "bad args") };
                let clamp = |n: usize| (a.clamp(0, n as i64) as usize, b.clamp(0, n as i64) as usize);
                match x {
                    Str(s) => {
                        let (a, b) = clamp(s.chars().count());
                        Str(s.chars().skip(a).take(b.saturating_sub(a)).collect())
                    }
                    List(xs) => {
                        let (a, b) = clamp(xs.len());
                        List(if a < b { xs[a..b].to_vec() } else { vec![] })
                    }
                    _ => return self.fail(FaultKind::Runtime, "bad args"),
                }
            }
            "index_of" => {
                let (Str(s), Str(sub)) = (nx(), nx()) else { return self.fail(FaultKind::Runtime, "bad args") };
                match s.find(&sub) {
                    Some(byte) => Variant("Some".into(), Fields::Pos(vec![Int(s[..byte].chars().count() as i64)])),
                    None => Variant("None".into(), Fields::None),
                }
            }
            "replace" => {
                let (Str(s), Str(from), Str(to)) = (nx(), nx(), nx()) else { return self.fail(FaultKind::Runtime, "bad args") };
                if from.is_empty() {
                    return self.fail(FaultKind::Runtime, "replace: empty pattern");
                }
                Str(s.replace(&from, &to))
            }
            "lines" => match nx() {
                Str(s) => List(s.lines().map(|l| Str(l.to_string())).collect()),
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            "repeat" => {
                let (Str(s), Int(n)) = (nx(), nx()) else { return self.fail(FaultKind::Runtime, "bad args") };
                if n < 0 || (s.len() as i64).saturating_mul(n) > 100_000_000 {
                    return self.fail(FaultKind::Runtime, format!("repeat: bad count {n}"));
                }
                Str(s.repeat(n as usize))
            }
            "to_upper" | "to_lower" => match nx() {
                Str(s) => Str(if name == "to_upper" { s.to_uppercase() } else { s.to_lowercase() }),
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            // true when the string is not empty and every character is of the class
            "is_digit" | "is_alpha" | "is_space" => match nx() {
                Str(s) => Bool(
                    !s.is_empty()
                        && s.chars().all(|c| match name {
                            "is_digit" => c.is_ascii_digit(),
                            "is_alpha" => c.is_alphabetic(),
                            _ => c.is_whitespace(),
                        }),
                ),
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            "ord" => match nx() {
                Str(s) if s.chars().count() == 1 => Int(s.chars().next().unwrap() as i64),
                Str(s) => return self.fail(FaultKind::Runtime, format!("ord needs a single character, got {s:?}")),
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            "chr" => match nx() {
                Int(n) => match u32::try_from(n).ok().and_then(char::from_u32) {
                    Some(c) => Str(c.to_string()),
                    None => return self.fail(FaultKind::Runtime, format!("chr: {n} is not a character")),
                },
                _ => return self.fail(FaultKind::Runtime, "bad args"),
            },
            _ => return self.fail(FaultKind::Runtime, format!("unknown builtin `{name}`")),
        })
    }

    // ---------------- evaluation ----------------

    /// Evaluate a function or lambda body, catching an early `return`.
    fn eval_body(&mut self, body: &Expr, s: &Scope) -> R<Value> {
        match self.eval(body, s) {
            Err(f) if f.kind == FaultKind::Return => Ok(self.returning.take().unwrap_or(Value::Unit)),
            r => r,
        }
    }

    fn early_return<T>(&mut self, v: Value) -> R<T> {
        self.returning = Some(v);
        Err(Fault { kind: FaultKind::Return, msg: "`return` outside a function".into(), stack: vec![] })
    }

    /// Run one iteration; `Ok(true)` means the loop should stop (`break`).
    fn loop_body(&mut self, b: &Expr, s: &Scope) -> R<bool> {
        match self.eval(b, s) {
            Ok(_) => Ok(false),
            Err(f) if f.kind == FaultKind::Break => Ok(true),
            Err(f) if f.kind == FaultKind::Continue => Ok(false),
            Err(f) => Err(f),
        }
    }

    /// After a call: copy the final values of `mut` parameters back into the
    /// caller's variables.
    fn write_back(&mut self, places: &[&Expr], s: &Scope) -> R<()> {
        for (i, v) in std::mem::take(&mut self.writeback) {
            if let Some(p) = places.get(i) {
                self.assign_place(p, v, s)?;
            }
        }
        Ok(())
    }

    fn assign_place(&mut self, place: &Expr, v: Value, s: &Scope) -> R<()> {
        let mut path = Vec::new();
        let mut root = place;
        while let Expr::Field(r, f) = root {
            path.push(f.clone());
            root = r;
        }
        path.reverse();
        let Expr::Var(x) = root else { return self.fail(FaultKind::Runtime, "can only assign to a variable or its fields") };
        let Some(base) = lookup(s, x) else { return self.fail(FaultKind::Runtime, format!("no variable `{x}`")) };
        let Some(new) = set_path(base, &path, v) else { return self.fail(FaultKind::Runtime, format!("cannot assign to `{}`", crate::printer::expr(place, 0))) };
        // field types may be refined: the changed value must still be valid
        if !path.is_empty() {
            let tname = match &new {
                Value::Record(n, _) => Some(n.clone()),
                Value::Variant(c, _) => self.env.ctors.get(c).cloned(),
                _ => None,
            };
            if let Some(t) = tname {
                self.enforce(&TypeExpr::Named(t, vec![]), &new, &format!("assigning `{}`", crate::printer::expr(place, 0)))?;
            }
        }
        assign(s, x, new);
        Ok(())
    }

    pub fn eval(&mut self, e: &Expr, s: &Scope) -> R<Value> {
        Ok(match e {
            Expr::Int(n) => Value::Int(*n),
            Expr::Float(f) => Value::Float(*f),
            Expr::Tuple(es) => Value::Tuple(es.iter().map(|x| self.eval(x, s)).collect::<R<_>>()?),
            Expr::Str(x) => Value::Str(x.clone()),
            Expr::Bool(b) => Value::Bool(*b),
            Expr::Unit => Value::Unit,
            Expr::Hole(h) => return self.fail(FaultKind::Hole, format!("reached unfilled hole `?{h}`")),
            Expr::Var(x) => match lookup(s, x) {
                Some(v) => v,
                None => Value::FnRef(x.clone()),
            },
            Expr::List(es) => Value::List(es.iter().map(|x| self.eval(x, s)).collect::<R<_>>()?),
            Expr::Ctor(c, args) => {
                let tname = self.env.ctors.get(c).cloned().unwrap_or_default();
                let is_record = matches!(self.env.types.get(&tname).map(|t| &t.body), Some(TypeBody::Record(_)));
                let is_distinct = self.env.types.get(&tname).is_some_and(|t| t.distinct);
                let v = match args {
                    CtorArgs::Positional(es) if is_distinct && es.len() == 1 => Value::Distinct(c.clone(), Box::new(self.eval(&es[0], s)?)),
                    CtorArgs::Update(base, fs) => match self.eval(base, s)? {
                        Value::Record(n, mut vals) => {
                            for (f, fe) in fs {
                                let nv = self.eval(fe, s)?;
                                if let Some(slot) = vals.iter_mut().find(|(k, _)| k == f) {
                                    slot.1 = nv;
                                }
                            }
                            Value::Record(n, vals)
                        }
                        other => return self.fail(FaultKind::Runtime, format!("cannot update {}", to_json(&other))),
                    },
                    CtorArgs::None => Value::Variant(c.clone(), Fields::None),
                    CtorArgs::Positional(es) => {
                        Value::Variant(c.clone(), Fields::Pos(es.iter().map(|x| self.eval(x, s)).collect::<R<_>>()?))
                    }
                    CtorArgs::Record(fs) => {
                        // store fields in declaration order
                        let order = self.env.variants_of(&tname, &[]).and_then(|vs| vs.into_iter().find(|v| &v.0 == c)).and_then(|v| v.1).unwrap_or_default();
                        let mut vals = Vec::new();
                        for f in &order {
                            let (_, fe) = fs.iter().find(|(n, _)| n == f).unwrap();
                            vals.push((f.clone(), self.eval(fe, s)?));
                        }
                        if is_record { Value::Record(c.clone(), vals) } else { Value::Variant(c.clone(), Fields::Rec(vals)) }
                    }
                };
                // Refinements on fields and on the type itself are checked at construction.
                // Fields whose type is a generic parameter are not known here and are skipped.
                self.enforce(&TypeExpr::Named(tname.clone(), vec![]), &v, &format!("constructing `{c}`"))?;
                v
            }
            Expr::Call(f, args) => {
                let fv = self.eval(f, s)?;
                let avs = args.iter().map(|a| self.eval(a, s)).collect::<R<Vec<_>>>()?;
                self.writeback.clear();
                let r = self.apply(fv, avs)?;
                let places: Vec<&Expr> = args.iter().collect();
                self.write_back(&places, s)?;
                r
            }
            Expr::Method(r, m, args) => {
                let mut avs = vec![self.eval(r, s)?];
                for a in args {
                    avs.push(self.eval(a, s)?);
                }
                let f = lookup(s, m).unwrap_or(Value::FnRef(m.clone()));
                self.writeback.clear();
                let out = self.apply(f, avs)?;
                let places: Vec<&Expr> = std::iter::once(&**r).chain(args.iter()).collect();
                self.write_back(&places, s)?;
                out
            }
            Expr::Field(r, f) => match self.eval(r, s)? {
                Value::Distinct(_, x) if f == "value" => *x,
                Value::Tuple(xs) => match f.parse::<usize>().ok().and_then(|i| xs.get(i).cloned()) {
                    Some(v) => v,
                    None => return self.fail(FaultKind::Runtime, format!("tuple has no element {f}")),
                },
                Value::Record(_, fs) | Value::Variant(_, Fields::Rec(fs)) => match fs.into_iter().find(|(n, _)| n == f) {
                    Some((_, v)) => v,
                    None => return self.fail(FaultKind::Runtime, format!("no field `{f}`")),
                },
                other => return self.fail(FaultKind::Runtime, format!("no field `{f}` on {}", to_json(&other))),
            },
            Expr::Lambda(ps, body) => Value::Closure(ps.iter().map(|p| p.name.clone()).collect(), Rc::new((**body).clone()), s.clone()),
            Expr::Bin(op, l, r) => {
                use BinOp::*;
                // short-circuiting operators
                match op {
                    And | Or | Implies => {
                        let lv = self.eval(l, s)? == Value::Bool(true);
                        let res = match op {
                            And => lv && self.eval(r, s)? == Value::Bool(true),
                            Or => lv || self.eval(r, s)? == Value::Bool(true),
                            _ => !lv || self.eval(r, s)? == Value::Bool(true),
                        };
                        return Ok(Value::Bool(res));
                    }
                    _ => {}
                }
                let (lv, rv) = (self.eval(l, s)?, self.eval(r, s)?);
                match (op, lv, rv) {
                    (Eq, a, b) => Value::Bool(a == b),
                    (Ne, a, b) => Value::Bool(a != b),
                    (Add, Value::Str(a), Value::Str(b)) => Value::Str(a + &b),
                    (Add, Value::List(mut a), Value::List(b)) => {
                        a.extend(b);
                        Value::List(a)
                    }
                    (op, Value::Float(a), Value::Float(b)) => {
                        let r = match op {
                            Add => a + b,
                            Sub => a - b,
                            Mul => a * b,
                            Div => a / b,
                            Mod => a.rem_euclid(b),
                            Lt => return Ok(Value::Bool(a < b)),
                            Le => return Ok(Value::Bool(a <= b)),
                            Gt => return Ok(Value::Bool(a > b)),
                            Ge => return Ok(Value::Bool(a >= b)),
                            _ => return self.fail(FaultKind::Runtime, "bad operands"),
                        };
                        // no NaN or infinity: they have no JSON form and break equality
                        if !r.is_finite() {
                            return self.fail(FaultKind::Runtime, format!("{a} {} {b} is not a finite number", op.symbol()));
                        }
                        Value::Float(r)
                    }
                    (op, Value::Int(a), Value::Int(b)) => {
                        let ov = || fault(FaultKind::Runtime, format!("integer overflow in {a} {} {b}", op.symbol()));
                        match op {
                            Add => Value::Int(a.checked_add(b).ok_or_else(ov)?),
                            Sub => Value::Int(a.checked_sub(b).ok_or_else(ov)?),
                            Mul => Value::Int(a.checked_mul(b).ok_or_else(ov)?),
                            Div | Mod if b == 0 => return self.fail(FaultKind::Runtime, "division by zero"),
                            Div => Value::Int(a.div_euclid(b)),
                            Mod => Value::Int(a.rem_euclid(b)),
                            Lt => Value::Bool(a < b),
                            Le => Value::Bool(a <= b),
                            Gt => Value::Bool(a > b),
                            Ge => Value::Bool(a >= b),
                            _ => return self.fail(FaultKind::Runtime, "bad operands"),
                        }
                    }
                    _ => return self.fail(FaultKind::Runtime, format!("bad operands for `{}`", op.symbol())),
                }
            }
            Expr::Un(UnOp::Neg, x) => match self.eval(x, s)? {
                Value::Int(n) => Value::Int(-n),
                Value::Float(f) => Value::Float(-f),
                _ => return self.fail(FaultKind::Runtime, "bad operand"),
            },
            Expr::Un(UnOp::Not, x) => Value::Bool(self.eval(x, s)? != Value::Bool(true)),
            Expr::Is(x, c) if !self.env.ctors.contains_key(c) && self.env.is_type_name(c) => {
                let v = self.eval(x, s)?;
                Value::Bool(self.value_in(&TypeExpr::Named(c.clone(), vec![]), &v)?)
            }
            Expr::Is(x, c) => Value::Bool(match self.eval(x, s)? {
                Value::Variant(n, _) | Value::Record(n, _) | Value::Distinct(n, _) => &n == c,
                _ => false,
            }),
            Expr::If(c, t, f) => {
                if self.eval(c, s)? == Value::Bool(true) { self.eval(t, s)? } else { self.eval(f, s)? }
            }
            Expr::Match(x, name, arms) => {
                let v = self.eval(x, s)?;
                let s = &match name {
                    Some(n) => extend(s, n, v.clone()),
                    None => s.clone(),
                };
                for a in arms {
                    if let Some(s2) = self.matches(&a.pat, &v, s) {
                        return self.eval(&a.body, &s2);
                    }
                }
                return self.fail(FaultKind::Runtime, format!("no match arm for {}", to_json(&v)));
            }
            Expr::Block(stmts, last) => {
                let mut sc = s.clone();
                for st in stmts {
                    match st {
                        Stmt::Let(p, ann, x) => {
                            let v = self.eval(x, &sc)?;
                            if let Some(t) = ann {
                                self.enforce(t, &v, "let binding")?;
                            }
                            match self.matches(p, &v, &sc) {
                                Some(s2) => sc = s2,
                                None => return self.fail(FaultKind::Runtime, format!("let pattern did not match {}", to_json(&v))),
                            }
                        }
                        Stmt::Var(n, ann, x) => {
                            let v = self.eval(x, &sc)?;
                            if let Some(t) = ann {
                                self.enforce(t, &v, "let binding")?;
                            }
                            sc = extend(&sc, n, v);
                        }
                        Stmt::Assign(place, x) => {
                            let v = self.eval(x, &sc)?;
                            self.assign_place(place, v, &sc)?;
                        }
                        Stmt::While(c, b) => {
                            let mut n = 0u64;
                            while self.eval(c, &sc)? == Value::Bool(true) {
                                n += 1;
                                if n > 1_000_000 {
                                    return self.fail(FaultKind::Runtime, "`while` ran more than 1,000,000 times");
                                }
                                if self.loop_body(b, &sc)? {
                                    break;
                                }
                            }
                        }
                        Stmt::Loop(b) => {
                            let mut n = 0u64;
                            loop {
                                n += 1;
                                if n > 1_000_000 {
                                    return self.fail(FaultKind::Runtime, "`loop` ran more than 1,000,000 times without `break`");
                                }
                                if self.loop_body(b, &sc)? {
                                    break;
                                }
                            }
                        }
                        Stmt::Break => return Err(Fault { kind: FaultKind::Break, msg: "break".into(), stack: vec![] }),
                        Stmt::Continue => return Err(Fault { kind: FaultKind::Continue, msg: "continue".into(), stack: vec![] }),
                        Stmt::For(p, it, b) => {
                            let items = match self.eval(it, &sc)? {
                                Value::List(xs) | Value::Set(xs) => xs,
                                Value::Dict(kvs) => kvs.into_iter().map(|(k, v)| Value::Tuple(vec![k, v])).collect(),
                                other => return self.fail(FaultKind::Runtime, format!("cannot loop over {}", to_json(&other))),
                            };
                            for item in items {
                                let Some(s2) = self.matches(p, &item, &sc) else {
                                    return self.fail(FaultKind::Runtime, format!("for-loop pattern did not match {}", to_json(&item)));
                                };
                                if self.loop_body(b, &s2)? {
                                    break;
                                }
                            }
                        }
                        Stmt::Expr(x) => {
                            self.eval(x, &sc)?;
                        }
                        Stmt::Comment(_) => {}
                        Stmt::Assert(x) => {
                            if self.eval(x, &sc)? != Value::Bool(true) {
                                let detail = match x {
                                    Expr::Bin(op, l, r) => format!(
                                        " (left = {}, right = {})",
                                        to_json(&self.eval(l, &sc)?),
                                        to_json(&self.eval(r, &sc)?)
                                    )
                                    .replace("left", &format!("left of {}", op.symbol())),
                                    _ => String::new(),
                                };
                                return self.fail(FaultKind::Assert, format!("assertion failed: {}{detail}", crate::printer::expr(x, 0)));
                            }
                        }
                    }
                }
                self.eval(last, &sc)?
            }
            Expr::Mock(t) => Value::Mock(t.clone()),
            Expr::Interp(parts) => {
                let mut out = String::new();
                for p in parts {
                    match p {
                        StrPart::Lit(l) => out += l,
                        StrPart::Expr(e) => out += &show(&self.eval(e, s)?),
                    }
                }
                Value::Str(out)
            }
            Expr::Return(x) => {
                let v = self.eval(x, s)?;
                return self.early_return(v);
            }
            Expr::Try(x, _) => match self.eval(x, s)? {
                Value::Variant(c, Fields::Pos(mut xs)) if (c == "Ok" || c == "Some") && xs.len() == 1 => xs.pop().unwrap(),
                v @ Value::Variant(..) => return self.early_return(v),
                other => return self.fail(FaultKind::Runtime, format!("`!` / `?` on {}, not a Result or Option", to_json(&other))),
            },
            Expr::Except(x, name, fb) => match self.eval(x, s)? {
                Value::Variant(c, Fields::Pos(mut xs)) if (c == "Ok" || c == "Some") && xs.len() == 1 => xs.pop().unwrap(),
                Value::Variant(c, Fields::Pos(mut xs)) if c == "Err" && xs.len() == 1 => {
                    let s2 = match name {
                        Some(n) => extend(s, n, xs.pop().unwrap()),
                        None => s.clone(),
                    };
                    self.eval(fb, &s2)?
                }
                Value::Variant(..) => self.eval(fb, s)?,
                other => return self.fail(FaultKind::Runtime, format!("`except` on {}, not a Result or Option", to_json(&other))),
            },
        })
    }

    fn matches(&mut self, p: &Pattern, v: &Value, s: &Scope) -> Option<Scope> {
        match (p, v) {
            (Pattern::Wild, _) => Some(s.clone()),
            (Pattern::Cond(c), v) => (self.eval(c, &extend(s, "it", v.clone())).ok()? == Value::Bool(true)).then(|| s.clone()),
            // a type's name matches the values of that type
            (Pattern::Ctor(c, CtorPat::None), v) if !self.env.ctors.contains_key(c) && self.env.is_type_name(c) => {
                self.value_in(&TypeExpr::Named(c.clone(), vec![]), v).ok()?.then(|| s.clone())
            }
            (Pattern::Bind(x), v) => Some(extend(s, x, v.clone())),
            (Pattern::Int(a), Value::Int(b)) if a == b => Some(s.clone()),
            (Pattern::Str(a), Value::Str(b)) if a == b => Some(s.clone()),
            (Pattern::Bool(a), Value::Bool(b)) if a == b => Some(s.clone()),
            (Pattern::Ctor(c, CtorPat::Positional(ps)), Value::Distinct(n, x)) if c == n && ps.len() == 1 => self.matches(&ps[0], x, s),
            (Pattern::Ctor(c, CtorPat::None), Value::Distinct(n, _)) if c == n => Some(s.clone()),
            (Pattern::Tuple(ps), Value::Tuple(xs)) if ps.len() == xs.len() => {
                let mut sc = s.clone();
                for (p, x) in ps.iter().zip(xs) {
                    sc = self.matches(p, x, &sc)?;
                }
                Some(sc)
            }
            (Pattern::List(items, rest), Value::List(xs)) => {
                if xs.len() < items.len() || (rest.is_none() && xs.len() != items.len()) {
                    return None;
                }
                let mut sc = s.clone();
                for (ip, x) in items.iter().zip(xs) {
                    sc = self.matches(ip, x, &sc)?;
                }
                if let Some(r) = rest {
                    if r != "_" {
                        sc = extend(&sc, r, Value::List(xs[items.len()..].to_vec()));
                    }
                }
                Some(sc)
            }
            (Pattern::Ctor(c, cp), Value::Variant(n, _) | Value::Record(n, _)) if c == n => {
                let fields = match v {
                    Value::Record(_, fs) => Fields::Rec(fs.clone()),
                    Value::Variant(_, f) => f.clone(),
                    _ => unreachable!(),
                };
                match (cp, &fields) {
                    (CtorPat::None, _) => Some(s.clone()),
                    (CtorPat::Positional(ps), Fields::Pos(xs)) => {
                        let mut sc = s.clone();
                        for (ip, x) in ps.iter().zip(xs) {
                            sc = self.matches(ip, x, &sc)?;
                        }
                        Some(sc)
                    }
                    (CtorPat::Record(fps, _), Fields::Rec(fs)) => {
                        let mut sc = s.clone();
                        for (fname, fp) in fps {
                            let (_, fv) = fs.iter().find(|(n, _)| n == fname)?;
                            sc = self.matches(fp, fv, &sc)?;
                        }
                        Some(sc)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    // ---------------- JSON input ----------------

    /// Parse JSON into a value of the given type, validating every refinement.
    pub fn from_json(&mut self, t: &TypeExpr, j: &Json, path: &str) -> Result<Value, String> {
        let v = self.from_json_raw(t, j, path)?;
        match self.check_type(t, &v) {
            Ok(None) => Ok(v),
            Ok(Some(e)) => Err(format!("{path}: {e}")),
            Err(f) => Err(format!("{path}: {}", f.msg)),
        }
    }

    fn from_json_raw(&mut self, t: &TypeExpr, j: &Json, path: &str) -> Result<Value, String> {
        let t = self.env.expand_alias(t);
        let bad = |what: &str| Err(format!("{path}: expected {what}, got {j}"));
        match &t {
            TypeExpr::Unit => Ok(Value::Unit),
            TypeExpr::Refined(b, _) => self.from_json_raw(b, j, path),
            // the first member the JSON is a valid value of
            TypeExpr::Union(ms) => {
                for m in ms {
                    if let Ok(v) = self.from_json(m, j, path) {
                        return Ok(v);
                    }
                }
                bad(&format!("a `{}`", crate::printer::ty(&t)))
            }
            TypeExpr::Named(n, args) if self.env.variant_parent(n).is_some() => {
                let p = self.env.variant_parent(n).unwrap();
                let v = self.from_json_raw(&TypeExpr::Named(p, args.clone()), j, path)?;
                if matches!(&v, Value::Variant(c, _) if c == n) { Ok(v) } else { bad(&format!("a `{}`", short(n))) }
            }
            TypeExpr::Fn(..) => bad("a function (not representable in JSON)"),
            TypeExpr::Named(n, args) => match n.as_str() {
                "Int" => j.as_i64().map(Value::Int).map_or_else(|| bad("an integer"), Ok),
                "Float" => j.as_f64().map(Value::Float).map_or_else(|| bad("a number"), Ok),
                "Tuple" => match j.as_array().filter(|a| a.len() == args.len()) {
                    Some(xs) => Ok(Value::Tuple(
                        args.iter().zip(xs).enumerate().map(|(i, (t, x))| self.from_json_raw(t, x, &format!("{path}.{i}"))).collect::<Result<_, _>>()?,
                    )),
                    None => bad(&format!("an array of {} values", args.len())),
                },
                "Set" => match j.as_array() {
                    Some(xs) => Ok(make_set(
                        xs.iter().enumerate().map(|(i, x)| self.from_json_raw(&args[0], x, &format!("{path}[{i}]"))).collect::<Result<_, _>>()?,
                    )),
                    None => bad("an array"),
                },
                "Dict" => {
                    let mut pairs = Vec::new();
                    if let Some(o) = j.as_object() {
                        for (k, v) in o {
                            let kv = self.from_json_raw(&args[0], &Json::String(k.clone()), &format!("{path} key {k:?}"))?;
                            pairs.push((kv, self.from_json_raw(&args[1], v, &format!("{path}[{k:?}]"))?));
                        }
                    } else if let Some(xs) = j.as_array() {
                        for (i, x) in xs.iter().enumerate() {
                            let Some([k, v]) = x.as_array().map(|a| a.as_slice()).and_then(|a| <&[Json; 2]>::try_from(a).ok()) else {
                                return Err(format!("{path}[{i}]: expected a [key, value] pair"));
                            };
                            pairs.push((self.from_json_raw(&args[0], k, &format!("{path}[{i}].key"))?, self.from_json_raw(&args[1], v, &format!("{path}[{i}].value"))?));
                        }
                    } else {
                        return bad("an object or an array of [key, value] pairs");
                    }
                    Ok(make_dict(pairs))
                }
                "Bool" => j.as_bool().map(Value::Bool).map_or_else(|| bad("a boolean"), Ok),
                "String" => j.as_str().map(|s| Value::Str(s.into())).map_or_else(|| bad("a string"), Ok),
                "List" => match j.as_array() {
                    Some(xs) => Ok(Value::List(
                        xs.iter().enumerate().map(|(i, x)| self.from_json_raw(&args[0], x, &format!("{path}[{i}]"))).collect::<Result<_, _>>()?,
                    )),
                    None => bad("an array"),
                },
                _ if self.env.types.get(n).is_some_and(|t| t.distinct) => {
                    let Some(v) = self.env.variants_of(n, args) else { return bad(n) };
                    let base = v[0].2[0].clone();
                    Ok(Value::Distinct(n.clone(), Box::new(self.from_json_raw(&base, j, path)?)))
                }
                _ => {
                    let Some(variants) = self.env.variants_of(n, args) else { return bad(n) };
                    let is_record = matches!(self.env.types[n].body, TypeBody::Record(_));
                    let (cname, payload) = if is_record {
                        (n.clone(), j)
                    } else if let Some(s) = j.as_str() {
                        (s.to_string(), &Json::Null)
                    } else if let Some(o) = j.as_object().filter(|o| o.len() == 1) {
                        let (k, v) = o.iter().next().unwrap();
                        (k.clone(), v)
                    } else {
                        let names: Vec<_> = variants.iter().map(|v| v.0.clone()).collect();
                        return bad(&format!("one of {}", names.join(" | ")));
                    };
                    let Some((cname, names, ftys)) = variants.iter().find(|v| v.0 == cname || short(&v.0) == cname).map(|v| (v.0.clone(), &v.1, &v.2)) else {
                        return Err(format!("{path}: `{cname}` is not a constructor of `{n}`"));
                    };
                    let p = format!("{path}.{cname}");
                    match names {
                        Some(names) => {
                            let Some(obj) = payload.as_object() else { return Err(format!("{p}: expected an object")) };
                            let mut fs = Vec::new();
                            for (fname, ft) in names.iter().zip(ftys) {
                                let fj = obj.get(fname).ok_or_else(|| format!("{p}: missing field `{fname}`"))?;
                                fs.push((fname.clone(), self.from_json_raw(ft, fj, &format!("{p}.{fname}"))?));
                            }
                            if let Some(extra) = obj.keys().find(|k| !names.contains(k)) {
                                return Err(format!("{p}: unknown field `{extra}`"));
                            }
                            Ok(if is_record { Value::Record(cname, fs) } else { Value::Variant(cname, Fields::Rec(fs)) })
                        }
                        None if ftys.is_empty() => Ok(Value::Variant(cname, Fields::None)),
                        None => {
                            let items: Vec<Json> = if ftys.len() == 1 { vec![payload.clone()] } else { payload.as_array().cloned().unwrap_or_default() };
                            if items.len() != ftys.len() {
                                return Err(format!("{p}: expected {} values", ftys.len()));
                            }
                            let vals = ftys.iter().zip(&items).enumerate().map(|(i, (ft, x))| self.from_json_raw(ft, x, &format!("{p}.{i}"))).collect::<Result<_, _>>()?;
                            Ok(Value::Variant(cname, Fields::Pos(vals)))
                        }
                    }
                }
            },
        }
    }
}

