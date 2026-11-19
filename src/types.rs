//! Static checking: Hindley–Milner style inference with explicit signatures
//! on top-level functions, effect checking, exhaustiveness checking, and
//! typed-hole reporting.

use crate::ast::*;
use crate::parser::Parser;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

pub const PRELUDE: &str = r#"
/// An optional value.
type Option[T] =
  | None
  | Some(T)

/// The result of an operation that can fail.
type Result[T, E] =
  | Ok(T)
  | Err(E)
"#;

/// Builtin functions and their type schemes.
pub const BUILTINS: &[(&str, &str, &str)] = &[
    ("len", "A", "(List[A]) -> Int"),
    ("map", "A,B", "(List[A], (A) -> B) -> List[B]"),
    ("filter", "A", "(List[A], (A) -> Bool) -> List[A]"),
    ("fold", "A,B", "(List[A], B, (B, A) -> B) -> B"),
    ("all", "A", "(List[A], (A) -> Bool) -> Bool"),
    ("any", "A", "(List[A], (A) -> Bool) -> Bool"),
    ("contains", "A", "(List[A], A) -> Bool"),
    ("push", "A", "(List[A], A) -> List[A]"),
    ("get", "A", "(List[A], Int) -> Option[A]"),
    ("sum", "", "(List[Int]) -> Int"),
    ("range", "", "(Int, Int) -> List[Int]"),
    ("abs", "", "(Int) -> Int"),
    ("min", "", "(Int, Int) -> Int"),
    ("max", "", "(Int, Int) -> Int"),
    ("matches", "", "(String, String) -> Bool"),
    ("str_len", "", "(String) -> Int"),
    ("starts_with", "", "(String, String) -> Bool"),
    ("to_string", "A", "(A) -> String"),
];

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Int,
    Bool,
    Str,
    Unit,
    Con(String, Vec<Ty>),
    Fn(Vec<Ty>, Box<Ty>),
    Var(usize),
    /// A type parameter of the declaration being checked.
    Rigid(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct Diag {
    pub decl: String,
    pub msg: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HoleInfo {
    pub decl: String,
    pub name: String,
    /// The type the hole must produce.
    pub goal: String,
    /// What is in scope at the hole.
    pub context: Vec<(String, String)>,
    /// Effects available at the hole.
    pub effects: Vec<String>,
    /// Contracts the hole's enclosing function must satisfy.
    pub obligations: Vec<String>,
}

#[derive(Default, Serialize)]
pub struct CheckReport {
    pub errors: Vec<Diag>,
    pub holes: Vec<HoleInfo>,
}

impl CheckReport {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Everything known about the declarations of a program, shared by the
/// checker, interpreter and generators.
pub struct Env {
    pub types: HashMap<String, TypeDecl>,
    /// ctor name -> (type name)
    pub ctors: HashMap<String, String>,
    pub effects: HashMap<String, EffectDecl>,
    pub fns: HashMap<String, FnDecl>,
}

impl Env {
    pub fn new(prog: &Program) -> (Env, Vec<Diag>) {
        let prelude = crate::parser::parse_program(PRELUDE).expect("prelude parses");
        let mut env = Env { types: HashMap::new(), ctors: HashMap::new(), effects: HashMap::new(), fns: HashMap::new() };
        let mut errs = Vec::new();
        for d in prelude.decls.iter().chain(prog.decls.iter()) {
            let name = d.name();
            let dup = match &d.kind {
                DeclKind::Type(t) => {
                    match &t.body {
                        TypeBody::Sum(vs) => {
                            for v in vs {
                                if env.ctors.insert(v.name.clone(), t.name.clone()).is_some() {
                                    errs.push(Diag { decl: name.clone(), msg: format!("constructor `{}` defined twice", v.name) });
                                }
                            }
                        }
                        TypeBody::Record(_) => {
                            env.ctors.insert(t.name.clone(), t.name.clone());
                        }
                        TypeBody::Alias(_) => {}
                    }
                    env.types.insert(t.name.clone(), t.clone()).is_some()
                }
                DeclKind::Effect(e) => env.effects.insert(e.name.clone(), e.clone()).is_some(),
                DeclKind::Fn(f) => env.fns.insert(f.name.clone(), f.clone()).is_some(),
                DeclKind::Test(_) => false,
            };
            if dup {
                errs.push(Diag { decl: name.clone(), msg: format!("`{name}` is defined twice") });
            }
        }
        (env, errs)
    }

    /// Follow refinement aliases to the underlying (unrefined) type expression.
    pub fn expand_alias(&self, t: &TypeExpr) -> TypeExpr {
        if let TypeExpr::Named(n, args) = t {
            if let Some(TypeDecl { body: TypeBody::Alias(a), params, .. }) = self.types.get(n) {
                let sub: HashMap<String, TypeExpr> = params.iter().cloned().zip(args.iter().cloned()).collect();
                return self.expand_alias(&subst_texpr(a, &sub));
            }
        }
        t.clone()
    }

    /// Constructors of a named type with type args substituted: (ctor, field names, field types)
    pub fn variants_of(&self, name: &str, args: &[TypeExpr]) -> Option<Vec<(String, Option<Vec<String>>, Vec<TypeExpr>)>> {
        let td = self.types.get(name)?;
        let sub: HashMap<String, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
        let s = |t: &TypeExpr| subst_texpr(t, &sub);
        match &td.body {
            TypeBody::Sum(vs) => Some(
                vs.iter()
                    .map(|v| match &v.payload {
                        Payload::None => (v.name.clone(), None, vec![]),
                        Payload::Positional(ts) => (v.name.clone(), None, ts.iter().map(s).collect()),
                        Payload::Record(fs) => (
                            v.name.clone(),
                            Some(fs.iter().map(|f| f.name.clone()).collect()),
                            fs.iter().map(|f| s(&f.ty)).collect(),
                        ),
                    })
                    .collect(),
            ),
            TypeBody::Record(fs) => Some(vec![(
                name.to_string(),
                Some(fs.iter().map(|f| f.name.clone()).collect()),
                fs.iter().map(|f| s(&f.ty)).collect(),
            )]),
            TypeBody::Alias(_) => None,
        }
    }
}

pub fn subst_texpr(t: &TypeExpr, sub: &HashMap<String, TypeExpr>) -> TypeExpr {
    match t {
        TypeExpr::Named(n, args) if args.is_empty() && sub.contains_key(n) => sub[n].clone(),
        TypeExpr::Named(n, args) => TypeExpr::Named(n.clone(), args.iter().map(|a| subst_texpr(a, sub)).collect()),
        TypeExpr::Fn(ps, r) => TypeExpr::Fn(ps.iter().map(|a| subst_texpr(a, sub)).collect(), Box::new(subst_texpr(r, sub))),
        TypeExpr::Unit => TypeExpr::Unit,
    }
}

pub fn parse_type_str(s: &str) -> TypeExpr {
    let mut p = Parser::from_str(s);
    p.type_expr().expect("builtin type parses")
}

// ------------------------------------------------------------------

struct Checker<'a> {
    env: &'a Env,
    subst: Vec<Option<Ty>>,
    scopes: Vec<HashMap<String, Ty>>,
    effects: BTreeSet<String>,
    rigid: BTreeSet<String>,
    decl: String,
    errors: Vec<Diag>,
    holes: Vec<(String, Ty, Vec<(String, Ty)>, Vec<String>)>,
    matches: Vec<(Ty, Vec<Pattern>)>,
}

impl<'a> Checker<'a> {
    fn err(&mut self, msg: impl Into<String>) {
        self.errors.push(Diag { decl: self.decl.clone(), msg: msg.into() });
    }

    fn fresh(&mut self) -> Ty {
        self.subst.push(None);
        Ty::Var(self.subst.len() - 1)
    }

    fn resolve(&self, t: &Ty) -> Ty {
        match t {
            Ty::Var(v) => match &self.subst[*v] {
                Some(t) => self.resolve(t),
                None => t.clone(),
            },
            Ty::Con(n, args) => Ty::Con(n.clone(), args.iter().map(|a| self.resolve(a)).collect()),
            Ty::Fn(ps, r) => Ty::Fn(ps.iter().map(|a| self.resolve(a)).collect(), Box::new(self.resolve(r))),
            _ => t.clone(),
        }
    }

    fn show(&self, t: &Ty) -> String {
        show_ty(&self.resolve(t))
    }

    fn occurs(&self, v: usize, t: &Ty) -> bool {
        match self.resolve(t) {
            Ty::Var(w) => v == w,
            Ty::Con(_, args) => args.iter().any(|a| self.occurs(v, a)),
            Ty::Fn(ps, r) => ps.iter().any(|a| self.occurs(v, a)) || self.occurs(v, &r),
            _ => false,
        }
    }

    fn unify(&mut self, a: &Ty, b: &Ty) -> Result<(), String> {
        let (a, b) = (self.resolve(a), self.resolve(b));
        match (&a, &b) {
            _ if a == b => Ok(()),
            (Ty::Var(v), t) | (t, Ty::Var(v)) => {
                if self.occurs(*v, t) {
                    return Err(format!("infinite type {} ~ {}", show_ty(&a), show_ty(&b)));
                }
                self.subst[*v] = Some(t.clone());
                Ok(())
            }
            (Ty::Con(n1, a1), Ty::Con(n2, a2)) if n1 == n2 && a1.len() == a2.len() => {
                for (x, y) in a1.iter().zip(a2) {
                    self.unify(x, y)?;
                }
                Ok(())
            }
            (Ty::Fn(p1, r1), Ty::Fn(p2, r2)) if p1.len() == p2.len() => {
                for (x, y) in p1.iter().zip(p2) {
                    self.unify(x, y)?;
                }
                self.unify(r1, r2)
            }
            _ => Err(format!("expected `{}`, found `{}`", show_ty(&a), show_ty(&b))),
        }
    }

    fn expect(&mut self, expected: &Ty, actual: &Ty, what: &str) {
        if let Err(e) = self.unify(expected, actual) {
            self.err(format!("{what}: {e}"));
        }
    }

    /// Convert a written type to a checker type. `vars` maps scheme params to fresh vars.
    fn lower(&mut self, t: &TypeExpr, vars: &HashMap<String, Ty>) -> Ty {
        match t {
            TypeExpr::Unit => Ty::Unit,
            TypeExpr::Fn(ps, r) => Ty::Fn(ps.iter().map(|p| self.lower(p, vars)).collect(), Box::new(self.lower(r, vars))),
            TypeExpr::Named(n, args) => {
                if let Some(v) = vars.get(n) {
                    return v.clone();
                }
                if self.rigid.contains(n) {
                    return Ty::Rigid(n.clone());
                }
                let largs: Vec<Ty> = args.iter().map(|a| self.lower(a, vars)).collect();
                match n.as_str() {
                    "Int" => Ty::Int,
                    "Bool" => Ty::Bool,
                    "String" => Ty::Str,
                    "Unit" => Ty::Unit,
                    "List" if largs.len() == 1 => Ty::Con("List".into(), largs),
                    _ => match self.env.types.get(n) {
                        Some(td) => {
                            if td.params.len() != largs.len() {
                                self.err(format!("type `{n}` expects {} type arguments, got {}", td.params.len(), largs.len()));
                            }
                            if let TypeBody::Alias(a) = &td.body {
                                let sub: HashMap<String, Ty> = td.params.iter().cloned().zip(largs).collect();
                                let a = a.clone();
                                return self.lower(&a, &sub);
                            }
                            Ty::Con(n.clone(), largs)
                        }
                        None => {
                            self.err(format!("unknown type `{n}`"));
                            self.fresh()
                        }
                    },
                }
            }
        }
    }

    fn instantiate(&mut self, params: &[String], t: &TypeExpr) -> Ty {
        let vars: HashMap<String, Ty> = params.iter().map(|p| (p.clone(), self.fresh())).collect();
        self.lower(t, &vars)
    }

    fn lookup(&self, x: &str) -> Option<Ty> {
        self.scopes.iter().rev().find_map(|s| s.get(x).cloned())
    }

    fn bind(&mut self, x: &str, t: Ty) {
        self.scopes.last_mut().unwrap().insert(x.to_string(), t);
    }

    fn fn_scheme(f: &FnDecl) -> TypeExpr {
        TypeExpr::Fn(f.params.iter().map(|p| p.ty.clone()).collect(), Box::new(f.ret.clone()))
    }

    /// Type of a named function value (top-level or builtin), freshly instantiated.
    fn global_fn(&mut self, name: &str) -> Option<Ty> {
        if let Some(f) = self.env.fns.get(name) {
            let f = f.clone();
            let missing: Vec<&String> = f.effects.iter().filter(|e| !self.effects.contains(*e)).collect();
            if !missing.is_empty() {
                let m = missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
                self.err(format!(
                    "`{name}` performs {{{m}}}, which is neither declared by `{}` nor handled here",
                    self.decl
                ));
            }
            return Some(self.instantiate(&f.type_params, &Self::fn_scheme(&f)));
        }
        if let Some((_, ps, sig)) = BUILTINS.iter().find(|(n, ..)| *n == name) {
            let ps: Vec<String> = ps.split(',').filter(|s| !s.is_empty()).map(String::from).collect();
            return Some(self.instantiate(&ps, &parse_type_str(sig)));
        }
        None
    }

    fn call(&mut self, fty: Ty, args: &[Expr], what: &str) -> Ty {
        let fty = self.resolve(&fty);
        let (ps, r) = match fty {
            Ty::Fn(ps, r) => (ps, *r),
            Ty::Var(_) => {
                let ps: Vec<Ty> = args.iter().map(|_| self.fresh()).collect();
                let r = self.fresh();
                self.expect(&fty, &Ty::Fn(ps.clone(), Box::new(r.clone())), what);
                (ps, r)
            }
            other => {
                self.err(format!("{what}: `{}` is not a function", show_ty(&other)));
                return self.fresh();
            }
        };
        if ps.len() != args.len() {
            self.err(format!("{what}: expects {} arguments, got {}", ps.len(), args.len()));
            return r;
        }
        for (i, (p, a)) in ps.iter().zip(args).enumerate() {
            let at = self.infer_with(a, Some(p));
            self.expect(p, &at, &format!("{what}, argument {}", i + 1));
        }
        r
    }

    fn infer_with(&mut self, e: &Expr, expected: Option<&Ty>) -> Ty {
        // Bidirectional lite: lambdas take their parameter types from context.
        if let (Expr::Lambda(ps, body), Some(exp)) = (e, expected) {
            if let Ty::Fn(pts, _) = self.resolve(exp) {
                if pts.len() == ps.len() {
                    self.scopes.push(HashMap::new());
                    for (p, t) in ps.iter().zip(pts.iter()) {
                        self.bind(p, t.clone());
                    }
                    let r = self.infer(body);
                    self.scopes.pop();
                    return Ty::Fn(pts, Box::new(r));
                }
            }
        }
        if let (Expr::Hole(h), Some(exp)) = (e, expected) {
            self.record_hole(h, exp.clone());
            return exp.clone();
        }
        self.infer(e)
    }

    fn record_hole(&mut self, name: &str, t: Ty) {
        let mut ctx: Vec<(String, Ty)> = Vec::new();
        for s in &self.scopes {
            for (k, v) in s {
                ctx.retain(|(n, _)| n != k);
                ctx.push((k.clone(), v.clone()));
            }
        }
        ctx.sort_by(|a, b| a.0.cmp(&b.0));
        self.holes.push((name.to_string(), t, ctx, self.effects.iter().cloned().collect()));
    }

    fn ctor_type(&mut self, c: &str) -> Option<(Ty, Vec<(Option<String>, Ty)>)> {
        let tname = self.env.ctors.get(c)?.clone();
        let td = self.env.types[&tname].clone();
        let vars: HashMap<String, Ty> = td.params.iter().map(|p| (p.clone(), self.fresh())).collect();
        let result = Ty::Con(tname.clone(), td.params.iter().map(|p| vars[p].clone()).collect());
        let fields = match &td.body {
            TypeBody::Record(fs) => fs.iter().map(|f| (Some(f.name.clone()), self.lower(&f.ty, &vars))).collect(),
            TypeBody::Sum(vs) => {
                let v = vs.iter().find(|v| v.name == c).unwrap();
                match &v.payload {
                    Payload::None => vec![],
                    Payload::Positional(ts) => ts.iter().map(|t| (None, self.lower(t, &vars))).collect(),
                    Payload::Record(fs) => fs.iter().map(|f| (Some(f.name.clone()), self.lower(&f.ty, &vars))).collect(),
                }
            }
            TypeBody::Alias(_) => vec![],
        };
        Some((result, fields))
    }

    fn infer(&mut self, e: &Expr) -> Ty {
        match e {
            Expr::Int(_) => Ty::Int,
            Expr::Str(_) => Ty::Str,
            Expr::Bool(_) => Ty::Bool,
            Expr::Unit => Ty::Unit,
            Expr::Hole(h) => {
                let t = self.fresh();
                self.record_hole(h, t.clone());
                t
            }
            Expr::Var(x) => {
                if let Some(t) = self.lookup(x) {
                    return t;
                }
                if let Some(t) = self.global_fn(x) {
                    return t;
                }
                self.err(format!("unknown variable `{x}`"));
                self.fresh()
            }
            Expr::List(es) => {
                let t = self.fresh();
                for el in es {
                    let et = self.infer(el);
                    self.expect(&t, &et, "list element");
                }
                Ty::Con("List".into(), vec![t])
            }
            Expr::Ctor(c, args) => {
                let Some((rt, fields)) = self.ctor_type(c) else {
                    self.err(format!("unknown constructor `{c}`"));
                    return self.fresh();
                };
                match args {
                    CtorArgs::None => {
                        if !fields.is_empty() {
                            self.err(format!("constructor `{c}` needs {} fields", fields.len()));
                        }
                    }
                    CtorArgs::Positional(es) => {
                        if fields.len() != es.len() || fields.iter().any(|f| f.0.is_some()) {
                            self.err(format!("constructor `{c}` expects {} positional arguments", fields.len()));
                        }
                        for ((_, ft), a) in fields.iter().zip(es) {
                            let at = self.infer_with(a, Some(ft));
                            self.expect(ft, &at, &format!("argument of `{c}`"));
                        }
                    }
                    CtorArgs::Record(fs) => {
                        for (fname, fe) in fs {
                            match fields.iter().find(|(n, _)| n.as_deref() == Some(fname)) {
                                Some((_, ft)) => {
                                    let ft = ft.clone();
                                    let at = self.infer_with(fe, Some(&ft));
                                    self.expect(&ft, &at, &format!("field `{c}.{fname}`"));
                                }
                                None => self.err(format!("`{c}` has no field `{fname}`")),
                            }
                        }
                        for (n, _) in &fields {
                            if let Some(n) = n {
                                if !fs.iter().any(|(f, _)| f == n) {
                                    self.err(format!("missing field `{n}` in `{c}`"));
                                }
                            }
                        }
                    }
                }
                rt
            }
            Expr::Call(f, args) => {
                let what = match &**f {
                    Expr::Var(n) => format!("call to `{n}`"),
                    _ => "call".into(),
                };
                let ft = self.infer(f);
                self.call(ft, args, &what)
            }
            Expr::Method(recv, m, args) => {
                let mut all = vec![(**recv).clone()];
                all.extend(args.iter().cloned());
                let Some(ft) = self.lookup(m).or_else(|| self.global_fn(m)) else {
                    self.err(format!("unknown function `{m}`"));
                    return self.fresh();
                };
                self.call(ft, &all, &format!("call to `{m}`"))
            }
            Expr::Perform(eff, op, args) => {
                let Some(ed) = self.env.effects.get(eff).cloned() else {
                    self.err(format!("unknown effect `{eff}`"));
                    return self.fresh();
                };
                let Some(sig) = ed.ops.iter().find(|o| &o.name == op) else {
                    self.err(format!("effect `{eff}` has no operation `{op}`"));
                    return self.fresh();
                };
                if !self.effects.contains(eff) {
                    self.err(format!(
                        "performs `{eff}.{op}` but `{eff}` is neither declared by `{}` nor handled here",
                        self.decl
                    ));
                }
                let ft = self.lower(&TypeExpr::Fn(sig.params.iter().map(|p| p.ty.clone()).collect(), Box::new(sig.ret.clone())), &HashMap::new());
                self.call(ft, args, &format!("`{eff}.{op}`"))
            }
            Expr::Field(r, f) => {
                let rt = self.infer(r);
                self.field_type(&rt, f)
            }
            Expr::Lambda(ps, body) => {
                self.scopes.push(HashMap::new());
                let pts: Vec<Ty> = ps.iter().map(|_| self.fresh()).collect();
                for (p, t) in ps.iter().zip(&pts) {
                    self.bind(p, t.clone());
                }
                let r = self.infer(body);
                self.scopes.pop();
                Ty::Fn(pts, Box::new(r))
            }
            Expr::Bin(op, l, r) => {
                let (lt, rt) = (self.infer(l), self.infer(r));
                use BinOp::*;
                match op {
                    Add | Sub | Mul | Div | Mod => {
                        self.expect(&Ty::Int, &lt, &format!("left of `{}`", op.symbol()));
                        self.expect(&Ty::Int, &rt, &format!("right of `{}`", op.symbol()));
                        Ty::Int
                    }
                    Concat => {
                        self.expect(&lt, &rt, "`++` operands");
                        match self.resolve(&lt) {
                            Ty::Str | Ty::Var(_) => {}
                            Ty::Con(n, _) if n == "List" => {}
                            t => self.err(format!("`++` works on String or List, found `{}`", show_ty(&t))),
                        }
                        lt
                    }
                    Eq | Ne => {
                        self.expect(&lt, &rt, &format!("`{}` operands", op.symbol()));
                        Ty::Bool
                    }
                    Lt | Le | Gt | Ge => {
                        self.expect(&Ty::Int, &lt, &format!("left of `{}`", op.symbol()));
                        self.expect(&Ty::Int, &rt, &format!("right of `{}`", op.symbol()));
                        Ty::Bool
                    }
                    And | Or | Implies => {
                        self.expect(&Ty::Bool, &lt, &format!("left of `{}`", op.symbol()));
                        self.expect(&Ty::Bool, &rt, &format!("right of `{}`", op.symbol()));
                        Ty::Bool
                    }
                }
            }
            Expr::Un(UnOp::Neg, x) => {
                let t = self.infer(x);
                self.expect(&Ty::Int, &t, "operand of `-`");
                Ty::Int
            }
            Expr::Un(UnOp::Not, x) => {
                let t = self.infer(x);
                self.expect(&Ty::Bool, &t, "operand of `!`");
                Ty::Bool
            }
            Expr::Is(x, c) => {
                let t = self.infer(x);
                match self.ctor_type(c) {
                    Some((ct, _)) => self.expect(&ct, &t, &format!("`is {c}`")),
                    None => self.err(format!("unknown constructor `{c}`")),
                }
                Ty::Bool
            }
            Expr::If(c, t, f) => {
                let ct = self.infer(c);
                self.expect(&Ty::Bool, &ct, "if condition");
                let tt = self.infer(t);
                let ft = self.infer_with(f, Some(&tt));
                self.expect(&tt, &ft, "if branches");
                tt
            }
            Expr::Match(s, arms) => {
                let st = self.infer(s);
                let rt = self.fresh();
                for a in arms {
                    self.scopes.push(HashMap::new());
                    self.pattern(&a.pat, &st);
                    let bt = self.infer_with(&a.body, Some(&rt));
                    self.expect(&rt, &bt, "match arms");
                    self.scopes.pop();
                }
                self.matches.push((st, arms.iter().map(|a| a.pat.clone()).collect()));
                rt
            }
            Expr::Block(stmts, last) => {
                self.scopes.push(HashMap::new());
                for s in stmts {
                    match s {
                        Stmt::Let(p, ann, e) => {
                            let ann = ann.as_ref().map(|a| self.lower(a, &HashMap::new()));
                            let t = self.infer_with(e, ann.as_ref());
                            if let Some(a) = &ann {
                                self.expect(a, &t, "let annotation");
                            }
                            self.pattern(p, &t);
                            if !irrefutable(p) {
                                self.matches.push((t, vec![p.clone()]));
                            }
                        }
                        Stmt::Expr(e) => {
                            self.infer(e);
                        }
                        Stmt::Assert(e) => {
                            let t = self.infer(e);
                            self.expect(&Ty::Bool, &t, "assert");
                        }
                    }
                }
                let t = self.infer(last);
                self.scopes.pop();
                t
            }
            Expr::Handle(body, handlers) => {
                let outer = self.effects.clone();
                for h in handlers {
                    let Some(ed) = self.env.effects.get(&h.effect).cloned() else {
                        self.err(format!("unknown effect `{}`", h.effect));
                        continue;
                    };
                    if let HandlerKind::Clauses(cs) = &h.kind {
                        for op in &ed.ops {
                            if !cs.iter().any(|c| c.op == op.name) {
                                self.err(format!("handler for `{}` is missing operation `{}`", h.effect, op.name));
                            }
                        }
                        for c in cs {
                            let Some(sig) = ed.ops.iter().find(|o| o.name == c.op) else {
                                self.err(format!("effect `{}` has no operation `{}`", h.effect, c.op));
                                continue;
                            };
                            if sig.params.len() != c.params.len() {
                                self.err(format!("`{}.{}` takes {} parameters", h.effect, c.op, sig.params.len()));
                            }
                            self.scopes.push(HashMap::new());
                            for (p, sp) in c.params.iter().zip(&sig.params) {
                                let t = self.lower(&sp.ty, &HashMap::new());
                                self.bind(p, t);
                            }
                            let rt = self.lower(&sig.ret, &HashMap::new());
                            let bt = self.infer_with(&c.body, Some(&rt));
                            self.expect(&rt, &bt, &format!("result of handler `{}.{}`", h.effect, c.op));
                            self.scopes.pop();
                        }
                    }
                }
                for h in handlers {
                    self.effects.insert(h.effect.clone());
                }
                let t = self.infer(body);
                self.effects = outer;
                t
            }
        }
    }

    fn field_type(&mut self, rt: &Ty, f: &str) -> Ty {
        let mut rt = self.resolve(rt);
        if let Ty::Var(_) = rt {
            // Unknown receiver: if exactly one record type has this field, use it.
            let cands: Vec<String> = self
                .env
                .types
                .values()
                .filter(|t| matches!(&t.body, TypeBody::Record(fs) if fs.iter().any(|x| x.name == f)))
                .map(|t| t.name.clone())
                .collect();
            if cands.len() == 1 {
                let (ct, _) = self.ctor_type(&cands[0]).unwrap();
                self.expect(&ct, &rt, "field access");
                rt = self.resolve(&ct);
            }
        }
        match &rt {
            Ty::Con(n, _) => {
                let n = n.clone();
                if let Some(TypeDecl { body: TypeBody::Record(_), .. }) = self.env.types.get(&n) {
                    let (ct, fields) = self.ctor_type(&n).unwrap();
                    self.expect(&ct, &rt, "field access");
                    if let Some((_, t)) = fields.iter().find(|(x, _)| x.as_deref() == Some(f)) {
                        return t.clone();
                    }
                }
                self.err(format!("type `{}` has no field `{f}` (match on variants to access their fields)", show_ty(&rt)));
                self.fresh()
            }
            _ => {
                self.err(format!("cannot access field `{f}` on `{}`", show_ty(&rt)));
                self.fresh()
            }
        }
    }

    fn pattern(&mut self, p: &Pattern, t: &Ty) {
        match p {
            Pattern::Wild => {}
            Pattern::Bind(x) => self.bind(x, t.clone()),
            Pattern::Int(_) => self.expect(&Ty::Int, t, "pattern"),
            Pattern::Str(_) => self.expect(&Ty::Str, t, "pattern"),
            Pattern::Bool(_) => self.expect(&Ty::Bool, t, "pattern"),
            Pattern::List(items, rest) => {
                let el = self.fresh();
                let lt = Ty::Con("List".into(), vec![el.clone()]);
                self.expect(&lt, t, "list pattern");
                for i in items {
                    self.pattern(i, &el);
                }
                if let Some(r) = rest {
                    if r != "_" {
                        self.bind(r, lt);
                    }
                }
            }
            Pattern::Ctor(c, cp) => {
                let Some((ct, fields)) = self.ctor_type(c) else {
                    self.err(format!("unknown constructor `{c}` in pattern"));
                    return;
                };
                self.expect(t, &ct, &format!("pattern `{c}`"));
                match cp {
                    CtorPat::None => {
                        if !fields.is_empty() {
                            self.err(format!("pattern `{c}` must bind its {} fields (use `{c}(..)` or `{c} {{ .. }}`)", fields.len()));
                        }
                    }
                    CtorPat::Positional(ps) => {
                        if ps.len() != fields.len() {
                            self.err(format!("pattern `{c}` expects {} fields, got {}", fields.len(), ps.len()));
                        }
                        for (sp, (_, ft)) in ps.iter().zip(fields.iter()) {
                            self.pattern(sp, ft);
                        }
                    }
                    CtorPat::Record(fs, open) => {
                        for (fname, sp) in fs {
                            match fields.iter().find(|(n, _)| n.as_deref() == Some(fname)) {
                                Some((_, ft)) => {
                                    let ft = ft.clone();
                                    self.pattern(sp, &ft)
                                }
                                None => self.err(format!("`{c}` has no field `{fname}`")),
                            }
                        }
                        if !open && fs.len() != fields.len() {
                            self.err(format!("pattern `{c} {{..}}` does not mention every field; add `..`"));
                        }
                    }
                }
            }
        }
    }
}

pub fn irrefutable(p: &Pattern) -> bool {
    matches!(p, Pattern::Wild | Pattern::Bind(_))
}

pub fn show_ty(t: &Ty) -> String {
    match t {
        Ty::Int => "Int".into(),
        Ty::Bool => "Bool".into(),
        Ty::Str => "String".into(),
        Ty::Unit => "()".into(),
        Ty::Var(v) => format!("?T{v}"),
        Ty::Rigid(n) => n.clone(),
        Ty::Con(n, a) if a.is_empty() => n.clone(),
        Ty::Con(n, a) => format!("{n}[{}]", a.iter().map(show_ty).collect::<Vec<_>>().join(", ")),
        Ty::Fn(ps, r) => format!("({}) -> {}", ps.iter().map(show_ty).collect::<Vec<_>>().join(", "), show_ty(r)),
    }
}

// ------------------------------------------------------------------
// Exhaustiveness (a small version of Maranget's usefulness algorithm)

#[derive(Clone)]
enum P {
    Any,
    C(String, Vec<P>),
    Lit,
}

fn norm(c: &Checker, p: &Pattern) -> P {
    match p {
        Pattern::Wild | Pattern::Bind(_) => P::Any,
        Pattern::Int(_) | Pattern::Str(_) => P::Lit,
        Pattern::Bool(b) => P::C(b.to_string(), vec![]),
        Pattern::List(items, rest) => {
            let mut acc = match rest {
                Some(_) => P::Any,
                None => P::C("[]".into(), vec![]),
            };
            for i in items.iter().rev() {
                acc = P::C("::".into(), vec![norm(c, i), acc]);
            }
            acc
        }
        Pattern::Ctor(name, cp) => {
            let tname = c.env.ctors.get(name).cloned().unwrap_or_default();
            let field_names: Vec<String> = c
                .env
                .variants_of(&tname, &[])
                .and_then(|vs| vs.into_iter().find(|v| &v.0 == name))
                .map(|v| v.1.unwrap_or_else(|| (0..v.2.len()).map(|i| i.to_string()).collect()))
                .unwrap_or_default();
            let subs = match cp {
                CtorPat::None => vec![P::Any; field_names.len()],
                CtorPat::Positional(ps) => ps.iter().map(|p| norm(c, p)).collect(),
                CtorPat::Record(fs, _) => field_names
                    .iter()
                    .map(|f| fs.iter().find(|(n, _)| n == f).map(|(_, p)| norm(c, p)).unwrap_or(P::Any))
                    .collect(),
            };
            P::C(name.clone(), subs)
        }
    }
}

impl Checker<'_> {
    /// The constructors of a type, or None if it has infinitely many values.
    fn signature(&mut self, t: &Ty) -> Option<Vec<(String, Vec<Ty>)>> {
        match self.resolve(t) {
            Ty::Bool => Some(vec![("true".into(), vec![]), ("false".into(), vec![])]),
            Ty::Unit => Some(vec![("()".into(), vec![])]),
            Ty::Con(n, args) if n == "List" => {
                let l = Ty::Con("List".into(), args.clone());
                Some(vec![("[]".into(), vec![]), ("::".into(), vec![args[0].clone(), l])])
            }
            Ty::Con(n, args) => {
                let td = self.env.types.get(&n)?.clone();
                let ctors: Vec<String> = match &td.body {
                    TypeBody::Sum(vs) => vs.iter().map(|v| v.name.clone()).collect(),
                    TypeBody::Record(_) => vec![n.clone()],
                    TypeBody::Alias(_) => return None,
                };
                let mut out = Vec::new();
                for c in ctors {
                    let (ct, fields) = self.ctor_type(&c)?;
                    let _ = self.unify(&ct, &Ty::Con(n.clone(), args.clone()));
                    out.push((c, fields.into_iter().map(|f| self.resolve(&f.1)).collect()));
                }
                Some(out)
            }
            _ => None,
        }
    }

    fn missing(&mut self, rows: &[Vec<P>], tys: &[Ty], depth: usize) -> Option<Vec<String>> {
        if tys.is_empty() {
            return if rows.is_empty() { Some(vec![]) } else { None };
        }
        if depth > 12 {
            return None;
        }
        let rest_tys = &tys[1..];
        match self.signature(&tys[0]) {
            Some(cs) => {
                for (c, arg_tys) in cs {
                    let n = arg_tys.len();
                    let spec: Vec<Vec<P>> = rows
                        .iter()
                        .filter_map(|r| match &r[0] {
                            P::Any => Some([vec![P::Any; n], r[1..].to_vec()].concat()),
                            P::C(name, subs) if *name == c => Some([subs.clone(), r[1..].to_vec()].concat()),
                            _ => None,
                        })
                        .collect();
                    let tys2 = [arg_tys.clone(), rest_tys.to_vec()].concat();
                    if let Some(w) = self.missing(&spec, &tys2, depth + 1) {
                        let (args, rest) = w.split_at(n);
                        let named = self.env.ctors.get(&c).and_then(|t| self.env.variants_of(t, &[])).and_then(|vs| vs.into_iter().find(|v| v.0 == c)).and_then(|v| v.1);
                        let shown = match c.as_str() {
                            "::" => format!("[{}, ..]", args[0]),
                            _ if n == 0 => c.clone(),
                            _ if named.is_some() && args.iter().all(|a| a == "_") => format!("{c} {{ .. }}"),
                            _ if named.is_some() => {
                                let ns = named.unwrap();
                                let parts: Vec<String> = ns.iter().zip(args).filter(|(_, a)| *a != "_").map(|(n, a)| format!("{n}: {a}")).collect();
                                format!("{c} {{ {}, .. }}", parts.join(", "))
                            }
                            _ => format!("{c}({})", args.join(", ")),
                        };
                        return Some([vec![shown], rest.to_vec()].concat());
                    }
                }
                None
            }
            None => {
                let def: Vec<Vec<P>> = rows.iter().filter(|r| matches!(r[0], P::Any)).map(|r| r[1..].to_vec()).collect();
                self.missing(&def, rest_tys, depth + 1).map(|w| [vec!["_".to_string()], w].concat())
            }
        }
    }

    fn check_matches(&mut self) {
        let ms = std::mem::take(&mut self.matches);
        for (t, pats) in ms {
            let rows: Vec<Vec<P>> = pats.iter().map(|p| vec![norm(self, p)]).collect();
            let _ = P::Lit;
            if let Some(w) = self.missing(&rows, &[t.clone()], 0) {
                self.err(format!("non-exhaustive match on `{}`: `{}` is not covered", self.show(&t), w[0]));
            }
        }
    }
}

// ------------------------------------------------------------------

pub fn check(prog: &Program) -> CheckReport {
    let (env, mut errors) = Env::new(prog);
    let mut holes = Vec::new();
    for d in &prog.decls {
        let mut c = Checker {
            env: &env,
            subst: Vec::new(),
            scopes: vec![HashMap::new()],
            effects: BTreeSet::new(),
            rigid: BTreeSet::new(),
            decl: d.name(),
            errors: Vec::new(),
            holes: Vec::new(),
            matches: Vec::new(),
        };
        let mut obligations = Vec::new();
        match &d.kind {
            DeclKind::Type(t) => {
                c.rigid = t.params.iter().cloned().collect();
                let self_ty = match &t.body {
                    TypeBody::Alias(a) => c.lower(a, &HashMap::new()),
                    TypeBody::Record(fs) => {
                        for f in fs {
                            c.lower(&f.ty, &HashMap::new());
                        }
                        Ty::Con(t.name.clone(), t.params.iter().map(|p| Ty::Rigid(p.clone())).collect())
                    }
                    TypeBody::Sum(vs) => {
                        for v in vs {
                            match &v.payload {
                                Payload::None => {}
                                Payload::Positional(ts) => ts.iter().for_each(|t| {
                                    c.lower(t, &HashMap::new());
                                }),
                                Payload::Record(fs) => fs.iter().for_each(|f| {
                                    c.lower(&f.ty, &HashMap::new());
                                }),
                            }
                        }
                        Ty::Con(t.name.clone(), t.params.iter().map(|p| Ty::Rigid(p.clone())).collect())
                    }
                };
                if let Some(r) = &t.refinement {
                    c.bind("it", self_ty);
                    let rt = c.infer(r);
                    c.expect(&Ty::Bool, &rt, "refinement");
                }
            }
            DeclKind::Effect(e) => {
                for op in &e.ops {
                    for p in &op.params {
                        c.lower(&p.ty, &HashMap::new());
                    }
                    c.lower(&op.ret, &HashMap::new());
                }
            }
            DeclKind::Fn(f) => {
                c.rigid = f.type_params.iter().cloned().collect();
                for e in &f.effects {
                    if !env.effects.contains_key(e) {
                        c.err(format!("unknown effect `{e}`"));
                    }
                }
                c.effects = f.effects.iter().cloned().collect();
                for p in &f.params {
                    let t = c.lower(&p.ty, &HashMap::new());
                    c.bind(&p.name, t);
                }
                let ret = c.lower(&f.ret, &HashMap::new());
                for r in &f.requires {
                    let t = c.infer(r);
                    c.expect(&Ty::Bool, &t, "requires clause");
                    obligations.push(format!("requires {}", crate::printer::expr(r, 0)));
                }
                let bt = c.infer_with(&f.body, Some(&ret));
                c.expect(&ret, &bt, "function body");
                c.scopes.push(HashMap::new());
                c.bind("it", ret.clone());
                let saved = std::mem::take(&mut c.effects);
                for en in &f.ensures {
                    let t = c.infer(en);
                    c.expect(&Ty::Bool, &t, "ensures clause");
                    obligations.push(format!("ensures {}", crate::printer::expr(en, 0)));
                }
                c.effects = saved;
                c.scopes.pop();
            }
            DeclKind::Test(t) => {
                c.infer(&t.body);
            }
        }
        c.check_matches();
        let declared_goal = match &d.kind {
            DeclKind::Fn(f) if matches!(f.body, Expr::Hole(_)) => Some(crate::printer::ty(&f.ret)),
            _ => None,
        };
        for (name, t, ctx, effs) in std::mem::take(&mut c.holes) {
            let inferred = c.show(&t);
            holes.push(HoleInfo {
                decl: d.name(),
                name,
                goal: match &declared_goal {
                    Some(g) if *g != inferred => format!("{g}  (= {inferred} + refinement)"),
                    _ => inferred,
                },
                context: ctx.iter().map(|(n, t)| (n.clone(), c.show(t))).collect(),
                effects: effs,
                obligations: obligations.clone(),
            });
        }
        errors.extend(c.errors);
    }
    CheckReport { errors, holes }
}
