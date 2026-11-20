//! Verification passes beyond type checking.
//!
//! * `static_refinements`: compile-time evaluation of refinements on literal
//!   arguments (`make_port(80)` is rejected before anything runs).
//! * `fuzz_fn`: property-based checking of every contract of a function with
//!   inputs generated from its parameter types.
//!
//! * `check_fn`: the layered check. Every contract becomes an SMT obligation
//!   (`smt.rs`); if the solver proves them all the function is `proven`.
//!   A counterexample from the solver is run in the interpreter, and becomes
//!   a failure only if it really fails. Whatever is left is fuzzed.

use crate::ast::*;
use crate::interp::{to_json, FaultKind, Interp};
use crate::types::{Diag, Env};
use serde::Serialize;
use serde_json::Value as Json;

fn literal(e: &Expr) -> bool {
    match e {
        Expr::Int(_) | Expr::BigInt(_) | Expr::Float(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Unit => true,
        Expr::List(xs) | Expr::Tuple(xs) => xs.iter().all(literal),
        Expr::Ctor(_, CtorArgs::None) => true,
        Expr::Ctor(_, CtorArgs::Positional(xs)) => xs.iter().all(literal),
        Expr::Ctor(_, CtorArgs::Record(fs)) => fs.iter().all(|(_, x)| literal(x)),
        _ => false,
    }
}

pub fn walk(e: &Expr, f: &mut impl FnMut(&Expr)) {
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
        Expr::Field(r, _) | Expr::Is(r, _) | Expr::Un(_, r) | Expr::Lambda(_, r) | Expr::Return(r) | Expr::Mut(r) | Expr::Try(r, _) => w(r),
        Expr::Except(r, _, fb) => {
            w(r);
            w(fb)
        }
        Expr::Interp(parts) => parts.iter().for_each(|p| {
            if let StrPart::Expr(e, _) = p {
                w(e)
            }
        }),
        Expr::Bin(_, l, r) => {
            w(l);
            w(r)
        }
        Expr::If(c, t, x) => {
            w(c);
            w(t);
            w(x)
        }
        Expr::Match(s, _, arms) => {
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
    let resolved = crate::types::elaborate(prog);
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
    /// The solver proved every check (listed).
    Proven {
        checks: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        terminates: Option<crate::termination::Termination>,
    },
    /// All generated cases satisfied every contract. `proven` checks were
    /// proven by the solver; `unproven` are the others, with the reason.
    Fuzzed {
        cases: usize,
        discarded: usize,
        #[serde(default)]
        proven: usize,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        unproven: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        terminates: Option<crate::termination::Termination>,
    },
    /// A contract was violated; here is a concrete counterexample, found by
    /// the solver (and confirmed by running it) or by the fuzzer.
    Counterexample {
        args: Json,
        kind: String,
        message: String,
        #[serde(default)]
        found_by: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        terminates: Option<crate::termination::Termination>,
    },
    /// The function (or something it calls) still contains a hole.
    Incomplete {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        terminates: Option<crate::termination::Termination>,
    },
    /// Could not be checked automatically.
    Skipped { reason: String },
}

impl Verdict {
    pub fn tag(&self) -> String {
        match self {
            Verdict::Proven { .. } => "proven".into(),
            Verdict::Fuzzed { cases, .. } => format!("fuzzed:{cases}"),
            Verdict::Counterexample { .. } => "COUNTEREXAMPLE".into(),
            Verdict::Incomplete { .. } => "incomplete".into(),
            Verdict::Skipped { .. } => "unchecked".into(),
        }
    }

    /// Attach the termination result.
    pub fn with_termination(mut self, t: Option<crate::termination::Termination>) -> Self {
        match &mut self {
            Verdict::Proven { terminates, .. } | Verdict::Fuzzed { terminates, .. } | Verdict::Counterexample { terminates, .. } | Verdict::Incomplete { terminates, .. } => *terminates = t,
            Verdict::Skipped { .. } => {}
        }
        self
    }

    pub fn termination(&self) -> Option<&crate::termination::Termination> {
        match self {
            Verdict::Proven { terminates, .. } | Verdict::Fuzzed { terminates, .. } | Verdict::Counterexample { terminates, .. } | Verdict::Incomplete { terminates, .. } => terminates.as_ref(),
            Verdict::Skipped { .. } => None,
        }
    }

    /// How bad a result is, to show one status per definition.
    pub fn rank(&self) -> u8 {
        match self {
            Verdict::Proven { .. } => 0,
            Verdict::Fuzzed { .. } => 1,
            Verdict::Skipped { .. } => 2,
            Verdict::Incomplete { .. } => 3,
            Verdict::Counterexample { .. } => 4,
        }
    }
}

fn has_hole(e: &Expr) -> bool {
    let mut found = false;
    walk(e, &mut |x| found |= matches!(x, Expr::Hole(_)));
    found
}

/// The layered check of a function (or of an implementation's method, when
/// `imp` is given): prove what the solver can, confirm its counterexamples by
/// running them, and fuzz the rest.
/// `cross_check`: how many generated cases a proven function is still run on.
/// A failure there means the prover itself is wrong, and is reported as such.
pub fn check_fn(env: &Env, f: &FnDecl, imp: Option<&ImplDecl>, solver: Option<&str>, transparent: &std::collections::HashMap<String, bool>, cases: usize, cross_check: usize, seed: u64) -> Verdict {
    let (decl, method) = match imp {
        None => (f.clone(), false),
        Some(imp) => {
            let mut d = for_impl(imp, f);
            // run through its trait: the name may be shared with another trait or the type's own method
            d.name = crate::types::trait_call(&imp.trait_name, &f.name);
            static_type_arg(imp, f, &mut d);
            // the trait's contract binds every implementation
            if let Some(tm) = env.traits.get(&imp.trait_name).and_then(|t| t.methods.iter().find(|m| m.decl.name == f.name)) {
                let tm = for_impl(imp, &tm.decl);
                d.requires.splice(0..0, tm.requires);
                d.ensures.splice(0..0, tm.ensures);
            }
            (d, true)
        }
    };
    let fuzz = |proven: usize, unproven: Vec<String>| match fuzz_call(env, &decl, cases, seed, method) {
        Verdict::Fuzzed { cases, discarded, .. } => Verdict::Fuzzed { cases, discarded, proven, unproven, terminates: None },
        other => other,
    };
    let Some(bin) = solver else {
        return fuzz(0, vec!["no SMT solver was found (install z3)".into()]);
    };
    if has_hole(&decl.body) {
        return fuzz(0, vec![]);
    }
    let mut vc = crate::smt::Vc::new(env);
    vc.transparent = transparent.clone();
    vc.function(&decl, &decl.name);
    let (mut proven, mut unproven) = (vec![], vec![]);
    for i in 0..vc.obligations.len() {
        if vc.obligations[i].termination {
            continue;
        }
        let what = vc.obligations[i].what.clone();
        match vc.solve(bin, i) {
            crate::smt::Outcome::Proven => proven.push(what),
            crate::smt::Outcome::Refuted(args, msg) => {
                let vals: Vec<crate::interp::Value> = args.into_iter().map(|a| a.1).collect();
                match run_case(Interp::new(env, seed), &decl, vals, method, true) {
                    Case::Fault(Verdict::Counterexample { args, kind, message, .. }) => {
                        return Verdict::Counterexample { args, kind, message, found_by: "solver".into(), terminates: None };
                    }
                    _ => unproven.push(format!("{what}: {msg}, but running it did not fail")),
                }
            }
            crate::smt::Outcome::Unknown(msg) => unproven.push(format!("{what}: {msg}")),
        }
    }
    for g in &vc.relied_on {
        proven.push(format!("(seeing through `{g}`, assuming it terminates as marked)"));
    }
    if unproven.is_empty() {
        // a safety net for the prover: proven code is still run a little
        if cross_check > 0 {
            if let Verdict::Counterexample { args, kind, message, .. } = fuzz_limited(env, &decl, cross_check, seed, method, Some(CROSS_CHECK_STEPS)) {
                return Verdict::Counterexample {
                    args,
                    kind,
                    message: format!("{message}. The solver had proven every check, so this is a bug in the prover: please report it"),
                    found_by: "cross-check".into(),
                    terminates: None,
                };
            }
        }
        return Verdict::Proven { checks: proven, terminates: None };
    }
    fuzz(proven.len(), unproven)
}

/// The SMT-LIB query of each obligation of a function (for `verify --smt`).
pub fn smt_queries(env: &Env, f: &FnDecl, transparent: &std::collections::HashMap<String, bool>) -> Vec<(String, String)> {
    let mut vc = crate::smt::Vc::new(env);
    vc.transparent = transparent.clone();
    vc.function(f, &f.name);
    (0..vc.obligations.len()).filter(|&i| !vc.obligations[i].termination).map(|i| (vc.obligations[i].what.clone(), vc.smtlib(i))).collect()
}

enum Case {
    /// The inputs are not valid for the function (type or `requires`).
    Discarded,
    /// The run went past its step budget: no conclusion either way.
    TooCostly,
    Ok,
    Hole(String),
    Fault(Verdict),
}

/// Run one call with the given arguments, as the fuzzer does. `validate`
/// checks the arguments against the parameter types first (generated
/// arguments are valid already).
fn run_case(mut it: Interp, f: &FnDecl, args: Vec<crate::interp::Value>, method: bool, validate: bool) -> Case {
    let mut scope: crate::interp::Scope = None;
    for (p, a) in f.params.iter().zip(&args) {
        if validate && !matches!(it.check_type(&p.ty, a), Ok(None)) {
            return Case::Discarded;
        }
        scope = Some(std::rc::Rc::new(crate::interp::Frame::new(&p.name, a.clone(), scope)));
    }
    // Inputs that fail the precondition are not the function's responsibility.
    if f.requires.iter().any(|r| !matches!(it.eval(r, &scope), Ok(crate::interp::Value::Bool(true)))) {
        return Case::Discarded;
    }
    let res = if method { it.call_method(&f.name, args.clone()) } else { it.call_fn(&f.name, args.clone()) };
    if it.exhausted() {
        return Case::TooCostly;
    }
    match res {
        Ok(_) => Case::Ok,
        Err(fault) if fault.kind == FaultKind::Hole => Case::Hole(format!("{} (via {})", fault.msg, short_stack(&fault.stack))),
        Err(fault) => Case::Fault(Verdict::Counterexample {
            args: Json::Array(args.iter().map(to_json).collect()),
            kind: format!("{:?}", fault.kind),
            message: format!("{} (stack: {})", fault.msg, short_stack(&fault.stack)),
            found_by: "fuzzer".into(),
            terminates: None,
        }),
    }
}


/// Replace `Self` (and the impl's type parameters, at `Int`) in a method
/// signature so its inputs can be generated.
pub fn for_impl(imp: &ImplDecl, f: &FnDecl) -> FnDecl {
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
    let mut d = for_impl(imp, decl);
    d.name = crate::types::trait_call(&imp.trait_name, method);
    static_type_arg(imp, decl, &mut d);
    fuzz_call(env, &d, cases, seed, true)
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
        terminates: false,
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
        // `Self.empty()` in a law means this implementation's
        let head = crate::types::texpr_head(&imp.for_type);
        match it.with_self(&head, |it| it.eval(&f.body, &scope)) {
            Ok(crate::interp::Value::Bool(true)) => {}
            Ok(_) => {
                let shown: Vec<String> = f.params.iter().zip(&args).map(|(p, a)| format!("{} = {}", p.name, to_json(a))).collect();
                return Verdict::Counterexample {
                    args: Json::Array(args.iter().map(to_json).collect()),
                    kind: "Law".into(),
                    message: format!("law `{}` does not hold for `{}` with {}", law.name, crate::printer::ty(&imp.for_type), shown.join(", ")),
                    found_by: "fuzzer".into(),
                    terminates: None,
                };
            }
            Err(fault) => {
                return Verdict::Counterexample {
                    args: Json::Array(args.iter().map(to_json).collect()),
                    kind: format!("{:?}", fault.kind),
                    message: format!("evaluating law `{}`: {}", law.name, fault.msg),
                    found_by: "fuzzer".into(),
                    terminates: None,
                };
            }
        }
    }
    Verdict::Fuzzed { cases, discarded: 0, proven: 0, unproven: vec![], terminates: None }
}

/// The step budget of one cross-check run: proven code can still be too
/// costly to run (Ackermann's function), which proves nothing either way.
const CROSS_CHECK_STEPS: u64 = 200_000;

fn fuzz_call(env: &Env, f: &FnDecl, cases: usize, seed: u64, method: bool) -> Verdict {
    fuzz_limited(env, f, cases, seed, method, None)
}

/// Fuzz with an optional step budget per case. With a budget, a case that
/// goes past a limit (steps or recursion depth) is set aside, not a failure.
fn fuzz_limited(env: &Env, f: &FnDecl, cases: usize, seed: u64, method: bool, budget: Option<u64>) -> Verdict {
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
        it.budget = budget;
        let args = match args {
            Ok(a) => a,
            Err(e) => return Verdict::Skipped { reason: e },
        };
        match run_case(it, f, args, method, false) {
            Case::Fault(Verdict::Counterexample { kind, .. }) if budget.is_some() && kind == "Limit" => discarded += 1,
            Case::TooCostly => discarded += 1,
            Case::Discarded => discarded += 1,
            Case::Ok => ok += 1,
            Case::Hole(message) => return Verdict::Incomplete { message, terminates: None },
            Case::Fault(v) => return v,
        }
    }
    if ok == 0 && discarded > 0 {
        return Verdict::Skipped { reason: format!("all {discarded} generated inputs violated the precondition") };
    }
    Verdict::Fuzzed { cases: ok, discarded, proven: 0, unproven: vec![], terminates: None }
}

/// A call stack for a message: deep ones (runaway recursion) are cut short.
fn short_stack(stack: &[String]) -> String {
    if stack.len() <= 8 {
        return stack.join(" -> ");
    }
    format!("{} -> … {} more … -> {}", stack[..4].join(" -> "), stack.len() - 6, stack[stack.len() - 2..].join(" -> "))
}

/// A trait method without `self` is called with the type first (`Trait::empty(type)`):
/// give its checked form that argument, so the generator supplies the type.
fn static_type_arg(imp: &ImplDecl, f: &FnDecl, d: &mut FnDecl) {
    if !crate::resolve::takes_self(f) {
        let ty = TypeExpr::Named(crate::types::TYPE_ARG.into(), vec![imp.for_type.clone()]);
        d.params.insert(0, Param { name: crate::types::type_arg("Self"), ty, mutable: false });
    }
}
