//! Verification passes beyond type checking.
//!
//! * `static_refinements`: compile-time evaluation of refinements on literal
//!   arguments (`make_port(80)` is rejected before anything runs).
//! * `fuzz_fn`: property-based checking of every contract of a function with
//!   inputs generated from its parameter types, effects handled by mocks.
//!
//! Neither is a proof. The plan is to put an SMT backend in front of both:
//! obligations it proves become `proven`; the rest fall back to these.

use crate::ast::*;
use crate::interp::{to_json, FaultKind, Interp};
use crate::types::{Diag, Env};
use serde::Serialize;
use serde_json::Value as Json;

fn literal(e: &Expr) -> bool {
    match e {
        Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Unit => true,
        Expr::List(xs) | Expr::Tuple(xs) => xs.iter().all(literal),
        Expr::Ctor(_, CtorArgs::None) => true,
        Expr::Ctor(_, CtorArgs::Positional(xs)) => xs.iter().all(literal),
        Expr::Ctor(_, CtorArgs::Record(fs)) => fs.iter().all(|(_, x)| literal(x)),
        _ => false,
    }
}

fn walk(e: &Expr, f: &mut impl FnMut(&Expr)) {
    f(e);
    let mut w = |x: &Expr| walk(x, f);
    match e {
        Expr::List(xs) | Expr::Tuple(xs) => xs.iter().for_each(&mut w),
        Expr::Ctor(_, CtorArgs::Positional(xs)) => xs.iter().for_each(&mut w),
        Expr::Ctor(_, CtorArgs::Record(fs)) => fs.iter().for_each(|(_, x)| w(x)),
        Expr::Ctor(_, CtorArgs::Update(b, fs)) => {
            w(b);
            fs.iter().for_each(|(_, x)| w(x))
        }
        Expr::Call(g, xs) => {
            w(g);
            xs.iter().for_each(&mut w)
        }
        Expr::Method(r, _, xs) => {
            w(r);
            xs.iter().for_each(&mut w)
        }
        Expr::Field(r, _) | Expr::Is(r, _) | Expr::Un(_, r) | Expr::Lambda(_, r) | Expr::Return(r) | Expr::Try(r, _) => w(r),
        Expr::Except(r, _, fb) => {
            w(r);
            w(fb)
        }
        Expr::Bin(_, l, r) => {
            w(l);
            w(r)
        }
        Expr::If(c, t, x) => {
            w(c);
            w(t);
            w(x)
        }
        Expr::Match(s, arms) => {
            w(s);
            arms.iter().for_each(|a| w(&a.body))
        }
        Expr::Block(stmts, last) => {
            for s in stmts {
                match s {
                    Stmt::Comment(_) => {}
                    Stmt::Let(_, _, x) | Stmt::Var(_, _, x) | Stmt::Expr(x) | Stmt::Assert(x) => w(x),
                    Stmt::Assign(a, b) | Stmt::While(a, b) | Stmt::For(_, a, b) => {
                        w(a);
                        w(b)
                    }
                    Stmt::Loop(b) => w(b),
                    Stmt::Break | Stmt::Continue => {}
                }
            }
            w(last)
        }
        _ => {}
    }
}

pub fn static_refinements(prog: &Program, env: &Env) -> Vec<Diag> {
    let resolved = crate::resolve::resolve(prog);
    let prog = &resolved.program;
    let mut out = Vec::new();
    let mut it = Interp::new(env, 0);
    for d in &prog.decls {
        let bodies: Vec<&Expr> = match &d.kind {
            DeclKind::Fn(f) => vec![&f.body],
            DeclKind::Test(t) => vec![&t.body],
            _ => vec![],
        };
        for b in bodies {
            walk(b, &mut |e| {
                let check = |it: &mut Interp, ty: &TypeExpr, arg: &Expr, ctx: String, out: &mut Vec<Diag>| {
                    if !literal(arg) {
                        return;
                    }
                    let Ok(v) = it.eval(arg, &None) else { return };
                    if let Ok(Some(msg)) = it.check_type(ty, &v) {
                        out.push(Diag { decl: d.name(), msg: format!("refinement violated at compile time in {ctx}: {msg}") });
                    }
                };
                match e {
                    Expr::Call(f, args) => {
                        if let Expr::Var(n) = &**f {
                            if let Some(fd) = env.fns.get(n) {
                                for (p, a) in fd.params.iter().zip(args) {
                                    check(&mut it, &p.ty, a, format!("argument `{}` of `{n}`", p.name), &mut out);
                                }
                            }
                        }
                    }
                    Expr::Ctor(c, CtorArgs::Positional(xs)) if env.types.get(c).is_some_and(|t| t.distinct) && xs.len() == 1 && literal(&xs[0]) => {
                        // constructing the value runs its checks
                        if let Err(fault) = it.eval(e, &None) {
                            out.push(Diag { decl: d.name(), msg: format!("refinement violated at compile time: {}", fault.msg) });
                        }
                    }
                    Expr::Ctor(c, CtorArgs::Record(fs)) => {
                        let tname = env.ctors.get(c).cloned().unwrap_or_default();
                        if let Some(vs) = env.variants_of(&tname, &[]) {
                            if let Some((_, Some(names), tys)) = vs.iter().find(|v| &v.0 == c) {
                                for (fname, fe) in fs {
                                    if let Some(i) = names.iter().position(|n| n == fname) {
                                        check(&mut it, &tys[i], fe, format!("field `{c}.{fname}`"), &mut out);
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            });
        }
    }
    out
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Verdict {
    /// All generated cases satisfied every contract.
    Fuzzed { cases: usize, discarded: usize },
    /// A contract was violated; here is a concrete counterexample.
    Counterexample { args: Json, effects: Json, kind: String, message: String },
    /// The function (or something it calls) still contains a hole.
    Incomplete { message: String },
    /// Could not be checked automatically.
    Skipped { reason: String },
}

impl Verdict {
    pub fn tag(&self) -> String {
        match self {
            Verdict::Fuzzed { cases, .. } => format!("fuzzed:{cases}"),
            Verdict::Counterexample { .. } => "COUNTEREXAMPLE".into(),
            Verdict::Incomplete { .. } => "incomplete".into(),
            Verdict::Skipped { .. } => "unchecked".into(),
        }
    }
}

pub fn fuzz_fn(env: &Env, f: &FnDecl, cases: usize, seed: u64) -> Verdict {
    fuzz_call(env, f, cases, seed, false)
}

/// Replace `Self` (and the impl's type parameters, at `Int`) in a method
/// signature so its inputs can be generated.
fn for_impl(imp: &ImplDecl, f: &FnDecl) -> FnDecl {
    let mut sub: std::collections::HashMap<String, TypeExpr> =
        imp.type_params.iter().map(|p| (p.clone(), TypeExpr::Named("Int".into(), vec![]))).collect();
    let self_ty = crate::types::subst_texpr(&imp.for_type, &sub);
    sub.insert("Self".into(), self_ty);
    let mut g = f.clone();
    for p in &mut g.params {
        p.ty = crate::types::subst_texpr(&p.ty, &sub);
    }
    g.ret = crate::types::subst_texpr(&g.ret, &sub);
    g
}

/// Fuzz every method of an implementation (the trait's contracts included).
pub fn fuzz_impl_method(env: &Env, imp: &ImplDecl, method: &str, cases: usize, seed: u64) -> Verdict {
    let tr = &env.traits[&imp.trait_name];
    let Some(decl) = imp.methods.iter().find(|m| m.name == method).or_else(|| tr.methods.iter().find(|m| m.decl.name == method).map(|m| &m.decl)) else {
        return Verdict::Skipped { reason: format!("no method `{method}`") };
    };
    fuzz_call(env, &for_impl(imp, decl), cases, seed, true)
}

/// Check a trait law for one implementation with generated inputs.
pub fn check_law(env: &Env, imp: &ImplDecl, law: &Law, cases: usize, seed: u64) -> Verdict {
    let as_fn = FnDecl {
        doc: vec![],
        comments: vec![],
        name: law.name.clone(),
        type_params: vec![],
        bounds: vec![],
        params: law.params.clone(),
        ret: TypeExpr::Named("Bool".into(), vec![]),
        requires: vec![],
        ensures: vec![],
        body: law.body.clone(),
    };
    let f = for_impl(imp, &as_fn);
    for i in 0..cases {
        let mut it = Interp::new(env, seed.wrapping_add(i as u64));
        let mut generator = std::mem::replace(&mut it.generator, crate::generate::Gen::new(0));
        let args: Result<Vec<crate::interp::Value>, String> = f.params.iter().map(|p| generator.value(env, &p.ty, &mut it)).collect();
        it.generator = generator;
        let args = match args {
            Ok(a) => a,
            Err(e) => return Verdict::Skipped { reason: e },
        };
        let mut scope: crate::interp::Scope = None;
        for (p, a) in f.params.iter().zip(&args) {
            scope = Some(std::rc::Rc::new(crate::interp::Frame::new(&p.name, a.clone(), scope)));
        }
        match it.eval(&f.body, &scope) {
            Ok(crate::interp::Value::Bool(true)) => {}
            Ok(_) => {
                let shown: Vec<String> = f.params.iter().zip(&args).map(|(p, a)| format!("{} = {}", p.name, to_json(a))).collect();
                return Verdict::Counterexample {
                    args: Json::Array(args.iter().map(to_json).collect()),
                    effects: Json::Array(vec![]),
                    kind: "Law".into(),
                    message: format!("law `{}` does not hold for `{}` with {}", law.name, crate::printer::ty(&imp.for_type), shown.join(", ")),
                };
            }
            Err(fault) => {
                return Verdict::Counterexample {
                    args: Json::Array(args.iter().map(to_json).collect()),
                    effects: Json::Array(vec![]),
                    kind: format!("{:?}", fault.kind),
                    message: format!("evaluating law `{}`: {}", law.name, fault.msg),
                };
            }
        }
    }
    Verdict::Fuzzed { cases, discarded: 0 }
}

fn fuzz_call(env: &Env, f: &FnDecl, cases: usize, seed: u64, method: bool) -> Verdict {
    // Generic functions are fuzzed at one instance: every type parameter becomes `Int`.
    // Generic functions are fuzzed at one consistent instance (see `test_instance`).
    let inst = if f.type_params.is_empty() {
        std::collections::HashMap::new()
    } else {
        match crate::types::test_instance(env, f) {
            Some(i) => i,
            None => return Verdict::Skipped { reason: "no combination of types satisfies the bounds".into() },
        }
    };
    if f.params.iter().any(|p| matches!(p.ty, TypeExpr::Fn(..))) {
        return Verdict::Skipped { reason: "higher-order parameters are not generated yet".into() };
    }
    let (mut ok, mut discarded) = (0, 0);
    for i in 0..cases * 4 {
        if ok >= cases {
            break;
        }
        let mut it = Interp::new(env, seed.wrapping_add(i as u64));
        let mut generator = std::mem::replace(&mut it.generator, crate::generate::Gen::new(0));
        let args: Result<Vec<crate::interp::Value>, String> = f.params.iter().map(|p| generator.value(env, &crate::types::subst_texpr(&p.ty, &inst), &mut it)).collect();
        it.generator = generator;
        let args = match args {
            Ok(a) => a,
            Err(e) => return Verdict::Skipped { reason: e },
        };
        // Discard inputs that fail the precondition: they are not the function's responsibility.
        let mut scope: crate::interp::Scope = None;
        for (p, a) in f.params.iter().zip(&args) {
            scope = Some(std::rc::Rc::new(crate::interp::Frame::new(&p.name, a.clone(), scope)));
        }
        if f.requires.iter().any(|r| !matches!(it.eval(r, &scope), Ok(crate::interp::Value::Bool(true)))) {
            discarded += 1;
            continue;
        }
        let res = if method { it.call_method(&f.name, args.clone()) } else { it.call_fn(&f.name, args.clone()) };
        match res {
            Ok(_) => ok += 1,
            Err(fault) if fault.kind == FaultKind::Hole => {
                return Verdict::Incomplete { message: format!("{} (via {})", fault.msg, fault.stack.join(" -> ")) };
            }
            Err(fault) => {
                return Verdict::Counterexample {
                    args: Json::Array(args.iter().map(to_json).collect()),
                    effects: Json::Array(it.effect_log.clone()),
                    kind: format!("{:?}", fault.kind),
                    message: format!("{} (stack: {})", fault.msg, fault.stack.join(" -> ")),
                };
            }
        }
    }
    if ok == 0 && discarded > 0 {
        return Verdict::Skipped { reason: format!("all {discarded} generated inputs violated the precondition") };
    }
    Verdict::Fuzzed { cases: ok, discarded }
}
