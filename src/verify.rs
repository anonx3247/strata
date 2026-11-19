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
        Expr::Int(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Unit => true,
        Expr::List(xs) => xs.iter().all(literal),
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
        Expr::List(xs) => xs.iter().for_each(&mut w),
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
        Expr::Perform(_, _, xs) => xs.iter().for_each(&mut w),
        Expr::Method(r, _, xs) => {
            w(r);
            xs.iter().for_each(&mut w)
        }
        Expr::Field(r, _) | Expr::Is(r, _) | Expr::Un(_, r) | Expr::Lambda(_, r) => w(r),
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
                    Stmt::Let(_, _, x) | Stmt::Expr(x) | Stmt::Assert(x) => w(x),
                }
            }
            w(last)
        }
        Expr::Handle(b, hs) => {
            w(b);
            for h in hs {
                if let HandlerKind::Clauses(cs) = &h.kind {
                    cs.iter().for_each(|c| w(&c.body));
                }
                if let Some((_, init)) = &h.state {
                    w(init);
                }
                if let Some((_, b)) = &h.on_return {
                    w(b);
                }
            }
        }
        _ => {}
    }
}

pub fn static_refinements(prog: &Program, env: &Env) -> Vec<Diag> {
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
    // Generic functions are fuzzed at one instance: every type parameter becomes `Int`.
    let inst: std::collections::HashMap<String, TypeExpr> =
        f.type_params.iter().map(|p| (p.clone(), TypeExpr::Named("Int".into(), vec![]))).collect();
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
        it.mock_effects(&f.effect_names());
        match it.call_fn(&f.name, args.clone()) {
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
