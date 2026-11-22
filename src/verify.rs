//! Verification passes beyond type checking, all running compiled code
//! (the interpreter isn't used).
//!
//! * `refinements_in`: a literal argument or field that breaks its type's
//!   refinement (`make_port(80)`) is an error before anything runs: each is
//!   a probe, `let x: T = <literal>`, compiled and run.
//! * `check_fn`: the layered check. Every contract becomes an SMT obligation
//!   (`smt.rs`); if the solver proves them all the function is `proven`.
//!   A counterexample from the solver is run (a probe calling the function
//!   with it), and becomes a failure only if it really fails. Whatever is
//!   left is fuzzed by compiled drivers (`llvm::compile_fuzz`): functions,
//!   and through `wrappers`, trait methods and laws.

use crate::ast::*;
use crate::llvm::jit::Fault;

use crate::types::{Diag, Env};
use serde::Serialize;

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

/// Whether a value of `t` can break a refinement: one is in it, or in the
/// types it names, their fields and variants. `None`: `t` names something
/// not known here (a type parameter), so it can't be checked alone.
fn refined(env: &Env, t: &TypeExpr, seen: &mut Vec<Symbol>) -> Option<bool> {
    Some(match t {
        TypeExpr::Unit | TypeExpr::Fn(..) => false,
        TypeExpr::Refined(..) => true,
        TypeExpr::Union(ms) => {
            let mut any = false;
            for m in ms {
                any |= refined(env, m, seen)?;
            }
            any
        }
        TypeExpr::Named(n, args) => {
            let mut any = false;
            for a in args {
                any |= refined(env, a, seen)?;
            }
            if matches!(n.as_str(), "Int" | "Float" | "Bool" | "String" | "Unit" | "List" | "Set" | "Dict" | "Tuple") || seen.contains(n) {
                return Some(any);
            }
            let td = env.types.get(n)?;
            if td.refinement.is_some() {
                return Some(true);
            }
            seen.push(*n);
            // a declaration's own parameters stand for the arguments, checked above
            let own = |t: &TypeExpr, seen: &mut Vec<Symbol>| match t {
                TypeExpr::Named(p, a) if a.is_empty() && td.params.contains(p) => Some(false),
                t => refined(env, t, seen),
            };
            let mut r = any;
            match &td.body {
                TypeBody::Alias(a) => r |= own(a, seen)?,
                TypeBody::Record(fs) => {
                    for f in fs {
                        r |= own(&f.ty, seen)?;
                    }
                }
                TypeBody::Sum(vs) => {
                    for v in vs {
                        match &v.payload {
                            Payload::Type(_) => unreachable!("a union member defined elsewhere is desugared away"),
                            Payload::None => {}
                            Payload::Positional(ts) => {
                                for t in ts {
                                    r |= own(t, seen)?;
                                }
                            }
                            Payload::Record(fs) => {
                                for f in fs {
                                    r |= own(&f.ty, seen)?;
                                }
                            }
                        }
                    }
                }
            }
            r
        }
    })
}

/// A function of compile-time code, `{ stmts; () }`.
fn probe(name: &str, stmts: Vec<Stmt>) -> Decl {
    let f = FnDecl {
        doc: vec![],
        comments: vec![],
        name: Symbol::new(name),
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
    Decl { comments: vec![], doc: vec![], kind: DeclKind::Fn(f), module: Symbol::new(""), public: false }
}

/// Run probes added to `prog`: each one's fault, if any (`Err` when they
/// can't be compiled).
fn run_probes(prog: &Program, probes: Vec<Decl>, budget: i64) -> Result<Vec<Option<Fault>>, String> {
    let names: Vec<String> = probes.iter().map(|d| d.name()).collect();
    let mut p = prog.clone();
    p.decls.extend(probes);
    let (ir, roots, shapes) = crate::llvm::compile_comptime(&p, &names)?;
    Ok(crate::llvm::jit::comptime(&ir, &roots, &shapes, budget)?.into_iter().map(|r| r.err()).collect())
}

/// Literal arguments and fields that break their types' refinements, found
/// while compiling: a probe for each, `let x: T = <literal>`, all compiled
/// as one module and run. (Nothing is reported when they can't be compiled.)
pub fn refinements_in(prog: &Program, compiled: &Program, env: &Env) -> Vec<Diag> {
    // (where, the probe's statement, how to report its fault)
    let mut sites: Vec<(String, Stmt, Option<String>)> = vec![];
    for d in &prog.decls {
        let bodies: Vec<&Expr> = match &d.kind {
            DeclKind::Fn(f) => vec![&f.body],
            DeclKind::Test(t) => vec![&t.body],
            _ => vec![],
        };
        for b in bodies {
            walk(b, |e| {
                let mut check = |ty: &TypeExpr, arg: &Expr, ctx: String| {
                    if literal(arg) && refined(env, ty, &mut vec![]) == Some(true) {
                        sites.push((d.name(), Stmt::Let(Pattern::Wild, Some(ty.clone()), arg.clone()), Some(ctx)));
                    }
                };
                match e {
                    Expr::Call(f, args) => {
                        if let Expr::Var(n) = &**f {
                            if let Some(fd) = env.fns.get(&n.sym()) {
                                // (a type parameter's isn't known here: `refined` says so)
                                for (p, a) in fd.params.iter().zip(args) {
                                    check(&p.ty, a, format!("argument `{}` of `{n}`", p.name));
                                }
                            }
                        }
                    }
                    // constructing the value runs its checks
                    Expr::Ctor(c, CtorArgs::Positional(xs)) if env.types.get(&c.sym()).is_some_and(|t| t.distinct) && xs.len() == 1 && literal(&xs[0]) => {
                        sites.push((d.name(), Stmt::Let(Pattern::Wild, None, e.clone()), None));
                    }
                    Expr::Ctor(c, CtorArgs::Record(fs)) => {
                        let tname = env.ctors.get(&c.sym()).cloned().unwrap_or_default();
                        if env.types.get(&tname).is_some_and(|t| t.params.is_empty()) {
                            if let Some(vs) = env.variants_of(&tname, &[]) {
                                if let Some((_, Some(names), tys)) = vs.iter().find(|v| &v.0 == c) {
                                    for (fname, fe) in fs {
                                        if let Some(i) = names.iter().position(|n| n == fname) {
                                            check(&tys[i], fe, format!("field `{c}.{fname}`"));
                                        }
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
    if sites.is_empty() {
        return vec![];
    }
    let probes = sites.iter().enumerate().map(|(i, (_, s, _))| probe(&format!("strata_verify_literal_{i}"), vec![s.clone()])).collect();
    let Ok(faults) = run_probes(compiled, probes, JIT_STEPS) else { return vec![] };
    let mut out = vec![];
    for ((decl, _, ctx), fault) in sites.into_iter().zip(faults) {
        let Some(f) = fault else { continue };
        let msg = f.message.strip_prefix("let binding: ").unwrap_or(&f.message).to_string();
        let msg = match ctx {
            Some(ctx) => format!("refinement violated at compile time in {ctx}: {msg}"),
            None => format!("refinement violated at compile time: {msg}"),
        };
        if !out.iter().any(|d: &Diag| d.decl == decl && d.msg == msg) {
            out.push(Diag { decl, msg });
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
        /// Checks true by the types alone (not listed): left out by `build` too.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        given: Vec<String>,
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
        /// The checks proven, and those not (refuted, unknown, stopped), by
        /// name: what `build` leaves out, and keeps (`proofs`).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        checks: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        open: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        terminates: Option<crate::termination::Termination>,
    },
    /// A contract was violated; here is a concrete counterexample, found by
    /// the solver (and confirmed by running it) or by the fuzzer.
    Counterexample {
        /// Each argument, as the code that writes it.
        args: Vec<String>,
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
    /// Trusted, not checked: code outside what the verifier can see (today,
    /// the standard library's real terminal, disk and process). Its contract
    /// is taken as true, and callers are checked against it as usual.
    Assumed { why: String },
    /// Could not be checked automatically.
    Skipped { reason: String },
}

impl Verdict {
    pub fn tag(&self) -> String {
        match self {
            Verdict::Proven { .. } => "proven".into(),
            Verdict::Assumed { .. } => "assumed".into(),
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
            Verdict::Assumed { .. } | Verdict::Skipped { .. } => {}
        }
        self
    }

    pub fn termination(&self) -> Option<&crate::termination::Termination> {
        match self {
            Verdict::Proven { terminates, .. } | Verdict::Fuzzed { terminates, .. } | Verdict::Counterexample { terminates, .. } | Verdict::Incomplete { terminates, .. } => terminates.as_ref(),
            Verdict::Assumed { .. } | Verdict::Skipped { .. } => None,
        }
    }

    /// How bad a result is, to show one status per definition.
    pub fn rank(&self) -> u8 {
        match self {
            Verdict::Proven { .. } => 0,
            Verdict::Assumed { .. } => 1,
            Verdict::Fuzzed { .. } => 2,
            Verdict::Skipped { .. } => 3,
            Verdict::Incomplete { .. } => 4,
            Verdict::Counterexample { .. } => 5,
        }
    }
}

/// Why a function can't be run here because it reaches foreign code
/// (`extern c`), which only compiled code calls: it is in a module that
/// declares externs (a native module), takes one of its types, or calls one of
/// its functions, directly or through other functions.
fn foreign(env: &Env, f: &FnDecl, imp: Option<&ImplDecl>) -> Option<String> {
    use crate::names::module_of;
    // a capability's own code reaches the outside world; one passed in can't be made here
    if let Some(i) = imp {
        let head = crate::types::texpr_head(&i.for_type);
        if env.types.get(&Symbol::new(&head)).is_some_and(|t| t.mock) {
            return Some(format!("`{}` is a mock, for tests only: its methods are checked by the tests that use it", crate::names::short(&head)));
        }
        if env.capabilities.contains(&Symbol::new(&head)) {
            return Some(format!("`{}` is a capability: it reaches the outside world (only `strata run` gives one)", crate::names::short(&head)));
        }
    }
    for p in &f.params {
        let mut named = Vec::new();
        crate::ast::Visit::ty(&mut Names(&mut named), &p.ty);
        if let Some(n) = named.iter().find(|n| env.capabilities.contains(*n)) {
            return Some(format!("it takes the capability `{}`, which only the runtime makes", crate::names::short(n)));
        }
    }
    if env.externs.is_empty() {
        return None;
    }
    let native: crate::fx::HashSet<&str> = env.externs.keys().map(|q| module_of(q)).collect();
    let shown = |m: &str| if m.is_empty() { "main".to_string() } else { m.to_string() };
    let home = match imp {
        Some(i) => module_of(&crate::types::texpr_head(&i.for_type)).to_string(),
        None => module_of(&f.name).to_string(),
    };
    if native.contains(home.as_str()) {
        return Some(format!("module `{}` calls foreign code (`extern`): only compiled code runs it", shown(&home)));
    }
    for p in &f.params {
        let mut named = Vec::new();
        crate::ast::Visit::ty(&mut Names(&mut named), &p.ty);
        if let Some(n) = named.iter().find(|n| native.contains(module_of(n))) {
            return Some(format!("it takes `{n}`, from module `{}`, which calls foreign code", shown(module_of(n))));
        }
    }
    // the functions it calls, and theirs
    let mut seen = crate::fx::HashSet::default();
    let mut todo = vec![f.body.clone()];
    while let Some(body) = todo.pop() {
        let mut called = None;
        crate::ast::walk(&body, |e| {
            if let Expr::Var(n) = e {
                if called.is_none() && env.fns.contains_key(n) && seen.insert(*n) {
                    if native.contains(module_of(n)) {
                        called = Some(*n);
                    } else if let Some(g) = env.fns.get(n) {
                        todo.push(g.body.clone());
                    }
                }
            }
        });
        if let Some(n) = called {
            return Some(format!("it calls `{n}`, in module `{}`, which calls foreign code: only compiled code runs it", shown(module_of(&n))));
        }
    }
    None
}

/// The type names in a type.
struct Names<'a>(&'a mut Vec<Symbol>);
impl crate::ast::Visit for Names<'_> {
    fn ty(&mut self, t: &TypeExpr) {
        if let TypeExpr::Named(n, _) = t {
            self.0.push(*n);
        }
        crate::ast::walk_ty(self, t)
    }
}

fn has_hole(e: &Expr) -> bool {
    crate::ast::any_expr(e, |x| matches!(x, Expr::Hole(_)))
}

/// The layered check of a function (or of an implementation's method, when
/// `imp` is given): prove what the solver can, confirm its counterexamples by
/// running them, and fuzz the rest. `key` names what is fuzzed: the function,
/// or the method's wrapper (`wrappers`).
/// `cross_check`: how many generated cases a proven function is still run on.
/// A failure there means the prover itself is wrong, and is reported as such.
#[allow(clippy::too_many_arguments)]
pub fn check_fn(env: &Env, f: &FnDecl, imp: Option<&ImplDecl>, key: &str, solver: Option<&crate::smt::Solver>, transparent: &crate::fx::HashMap<String, bool>, cases: usize, cross_check: usize, seed: u64) -> Verdict {
    // the real terminal, disk and process: trusted, never run here (only `strata run` reaches the system)
    if let Some(why) = foreign(env, f, imp) {
        return Verdict::Assumed { why };
    }
    let decl = match imp {
        None => f.clone(),
        Some(imp) => {
            let mut d = method_decl(env, imp, f);
            // run through its trait: the name may be shared with another trait or the type's own method
            d.name = Symbol::from(crate::types::trait_call(&imp.trait_name, &f.name));
            static_type_arg(env, imp, f, &mut d);
            d
        }
    };
    let shown = decl.name.to_string();
    let fuzz = |proven: usize, unproven: Vec<String>| match fuzz_limited(key, &shown, cases, seed, None) {
        Verdict::Fuzzed { cases, discarded, .. } => Verdict::Fuzzed { cases, discarded, proven, unproven, checks: vec![], open: vec![], terminates: None },
        // not fuzzed either: say what the solver couldn't prove
        Verdict::Skipped { reason } if !unproven.is_empty() => Verdict::Skipped { reason: format!("{reason}; not proven: {}", unproven.join("; ")) },
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
    vc.solver = Some(bin.clone());
    vc.function(&decl, &decl.name);
    if !RACE.load(std::sync::atomic::Ordering::Relaxed) {
        return solve_then_fuzz(&vc, bin, key, &shown, &decl, imp.is_some(), cases, cross_check, seed);
    }
    race(&vc, bin, key, &shown, &decl, imp.is_some(), cases, cross_check, seed)
}

/// Race the fuzzer against the solver (`race`); `verify --no-race` turns it
/// off: the solver first, then the fuzzer, so every result is the same from
/// run to run and the cross-check can tell a bug in the prover.
/// How long the solver works alone before the fuzzer races it.
const HEAD_START: std::time::Duration = std::time::Duration::from_millis(300);

pub static RACE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// The solver, then the fuzzer: for what the solver doesn't prove, or as a
/// cross-check of what it does.
#[allow(clippy::too_many_arguments)]
fn solve_then_fuzz(vc: &crate::smt::Vc, bin: &crate::smt::Solver, key: &str, shown: &str, decl: &FnDecl, method: bool, cases: usize, cross_check: usize, seed: u64) -> Verdict {
    let (mut proven, mut unproven, mut open) = (vec![], vec![], vec![]);
    // every obligation at once (each a solver process), then read in order
    let outcomes: Vec<Option<crate::smt::Outcome>> = std::thread::scope(|sc| {
        let handles: Vec<_> = (0..vc.obligations.len()).map(|i| (!vc.obligations[i].termination).then(|| sc.spawn(move || vc.solve(bin, i)))).collect();
        handles.into_iter().map(|h| h.map(|h| h.join().expect("a solver thread"))).collect()
    });
    for (i, out) in outcomes.into_iter().enumerate() {
        let Some(out) = out else { continue };
        let what = vc.obligations[i].what.clone();
        match out {
            crate::smt::Outcome::Proven => proven.push(what),
            crate::smt::Outcome::Refuted(args, msg) => {
                let vals: Vec<Expr> = args.into_iter().map(|a| a.1).collect();
                match replay(key, shown, decl, method, &vals) {
                    Some(Verdict::Counterexample { args, kind, message, .. }) => {
                        return Verdict::Counterexample { args, kind, message, found_by: "solver".into(), terminates: None };
                    }
                    _ => {
                        unproven.push(format!("{what}: {msg}, but running it did not fail"));
                        open.push(what);
                    }
                }
            }
            crate::smt::Outcome::Unknown(msg) => {
                unproven.push(format!("{what}: {msg}"));
                open.push(what);
            }
            crate::smt::Outcome::Stopped => {
                unproven.push(format!("{what}: stopped"));
                open.push(what);
            }
        }
    }
    for g in &vc.relied_on {
        proven.push(format!("(seeing through `{g}`, assuming it terminates as marked)"));
    }
    proven.extend(vc.invariants.iter().cloned());
    if unproven.is_empty() {
        // a safety net for the prover: proven code is still run a little
        if cross_check > 0 {
            if let Verdict::Counterexample { args, kind, message, .. } = fuzz_limited(key, shown, cross_check, seed, Some(CROSS_CHECK_STEPS)) {
                return Verdict::Counterexample { args, kind, message: prover_bug(&message), found_by: "cross-check".into(), terminates: None };
            }
        }
        return Verdict::Proven { checks: proven, given: vc.given.clone(), terminates: None };
    }
    match or_solver_inputs(fuzz_limited(key, shown, cases, seed, None), vc, bin, key, shown, decl, method, cases, seed) {
        Verdict::Fuzzed { cases, discarded, .. } => Verdict::Fuzzed { cases, discarded, proven: proven.len(), unproven, checks: proven.iter().chain(&vc.given).cloned().collect(), open, terminates: None },
        Verdict::Skipped { reason } if !unproven.is_empty() => Verdict::Skipped { reason: format!("{reason}; not proven: {}", unproven.join("; ")) },
        other => other,
    }
}

/// What a failure of code the solver proved means: a fault in a function it
/// calls is that function's (the prover relies on its contract, which
/// doesn't rule the fault out); one in its own code is the prover's.
fn prover_bug(message: &str) -> String {
    let frames: Vec<&str> = message.rsplit_once("(stack: ").and_then(|(_, s)| s.strip_suffix(')')).map(|s| s.split(" -> ").collect()).unwrap_or_default();
    match frames.as_slice() {
        [first, .., last] if first != last => format!("{message}. It fails in `{last}`, which it calls: see `{last}`'s own verdict"),
        _ => format!("{message}. The solver had proven every check, so this is a bug in the prover: please report it"),
    }
}

/// The solver and the fuzzer at once. The fuzzer runs the cases an
/// unproven function gets; the solver solves every obligation at once.
/// - The fuzzer finds a failure first: the solver is stopped, and the
///   failure is the verdict.
/// - The solver proves everything: the fuzzer is cut down to the
///   cross-check's cases (none when the function may not terminate); a
///   failure there, unless it only ran out of steps, is a bug in the prover.
/// - The solver's counterexample, confirmed by running it: the verdict, and
///   the fuzzer stops.
/// - Otherwise the fuzzer's verdict, with what was proven.
/// Which finishes first depends on timing; the inputs each finds don't.
#[allow(clippy::too_many_arguments)]
fn race(vc: &crate::smt::Vc, bin: &crate::smt::Solver, key: &str, shown: &str, decl: &FnDecl, method: bool, cases: usize, cross_check: usize, seed: u64) -> Verdict {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
    let stop_solver = AtomicBool::new(false);
    let solved = AtomicBool::new(false);
    let cap = AtomicUsize::new(cases);
    // a cross-check's steps until the solver has answered
    let budget = std::sync::atomic::AtomicU64::new(CROSS_CHECK_STEPS);
    std::thread::scope(|sc| {
        let fuzzer = sc.spawn(|| {
            // most proofs take a few milliseconds: the fuzzer only races a
            // solver still at work after HEAD_START (else it would take the
            // cores from solvers about to finish)
            let start = std::time::Instant::now();
            while !solved.load(Relaxed) && start.elapsed() < HEAD_START {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            let v = fuzz_capped(key, shown, cases, seed, &budget, &cap);
            if matches!(v, Verdict::Counterexample { .. }) {
                stop_solver.store(true, Relaxed);
            }
            v
        });
        let handles: Vec<_> = (0..vc.obligations.len()).map(|i| {
            let stop = &stop_solver;
            (!vc.obligations[i].termination).then(|| sc.spawn(move || vc.solve_until(bin, i, stop)))
        }).collect();
        let outcomes: Vec<Option<crate::smt::Outcome>> = handles.into_iter().map(|h| h.map(|h| h.join().expect("a solver thread"))).collect();
        // (the fuzzer is told only once the cases it may run are settled, below)
        let (mut proven, mut unproven, mut open) = (vec![], vec![], vec![]);
        let mut stopped = false;
        for (i, out) in outcomes.into_iter().enumerate() {
            let Some(out) = out else { continue };
            let what = vc.obligations[i].what.clone();
            match out {
                crate::smt::Outcome::Proven => proven.push(what),
                crate::smt::Outcome::Refuted(args, msg) => {
                    let vals: Vec<Expr> = args.into_iter().map(|a| a.1).collect();
                    match replay(key, shown, decl, method, &vals) {
                        Some(Verdict::Counterexample { args, kind, message, .. }) if !stop_solver.load(Relaxed) => {
                            cap.store(0, Relaxed);
                            solved.store(true, Relaxed);
                            let _ = fuzzer.join();
                            return Verdict::Counterexample { args, kind, message, found_by: "solver".into(), terminates: None };
                        }
                        _ => {
                        unproven.push(format!("{what}: {msg}, but running it did not fail"));
                        open.push(what);
                    }
                    }
                }
                crate::smt::Outcome::Unknown(msg) => {
                    unproven.push(format!("{what}: {msg}"));
                    open.push(what);
                }
                crate::smt::Outcome::Stopped => {
                    stopped = true;
                    open.push(what);
                }
            }
        }
        for g in &vc.relied_on {
            proven.push(format!("(seeing through `{g}`, assuming it terminates as marked)"));
        }
        proven.extend(vc.invariants.iter().cloned());
        if unproven.is_empty() && !stopped {
            // a safety net for the prover: proven code is still run a little
            cap.store(cross_check, Relaxed);
            solved.store(true, Relaxed);
            let v = fuzzer.join().expect("the fuzzer");
            if let Verdict::Counterexample { args, kind, message, .. } = v {
                if cross_check > 0 && kind != "Limit" {
                    return Verdict::Counterexample { args, kind, message: prover_bug(&message), found_by: "cross-check".into(), terminates: None };
                }
            }
            return Verdict::Proven { checks: proven, given: vc.given.clone(), terminates: None };
        }
        budget.store(FULL, Relaxed);
        solved.store(true, Relaxed);
        let fuzzed = or_solver_inputs(fuzzer.join().expect("the fuzzer"), vc, bin, key, shown, decl, method, cases, seed);
        match fuzzed {
            Verdict::Fuzzed { cases, discarded, .. } => Verdict::Fuzzed { cases, discarded, proven: proven.len(), unproven, checks: proven.iter().chain(&vc.given).cloned().collect(), open, terminates: None },
            Verdict::Skipped { reason } if !unproven.is_empty() => Verdict::Skipped { reason: format!("{reason}; not proven: {}", unproven.join("; ")) },
            other => other,
        }
    })
}

/// The SMT-LIB query of each obligation of a function (for `verify --smt`).
pub fn smt_queries(env: &Env, f: &FnDecl, transparent: &crate::fx::HashMap<String, bool>) -> Vec<(String, String)> {
    let mut vc = crate::smt::Vc::new(env);
    vc.transparent = transparent.clone();
    vc.function(f, &f.name);
    (0..vc.obligations.len()).filter(|&i| !vc.obligations[i].termination).map(|i| (vc.obligations[i].what.clone(), vc.smtlib(i))).collect()
}

/// Run what `key` names on the solver's counterexample, compiled: a probe
/// binding each value to its parameter's type (an invalid one is a fault
/// there, of the probe's own: the input is discarded), then calling it. Its
/// own precondition unmet discards it too. `None`: nothing to conclude.
fn replay(key: &str, shown: &str, decl: &FnDecl, method: bool, vals: &[Expr]) -> Option<Verdict> {
    match replay_all(key, shown, decl, method, &[vals.to_vec()])?.pop()? {
        Ran::Failed(v) => Some(v),
        _ => None,
    }
}

/// How many inputs the solver makes for a function random values can't be
/// made for (each a solver query).
const SOLVER_INPUTS: usize = 20;

/// The fuzzer's verdict; or, when it couldn't make inputs (a refinement
/// random values rarely meet: a sorted list, a record whose fields depend on
/// each other), the function run on inputs the solver makes.
#[allow(clippy::too_many_arguments)]
fn or_solver_inputs(v: Verdict, vc: &crate::smt::Vc, bin: &crate::smt::Solver, key: &str, shown: &str, decl: &FnDecl, method: bool, cases: usize, seed: u64) -> Verdict {
    let gave_up = matches!(&v, Verdict::Skipped { reason } if reason.contains("could not generate") || reason.contains("generated inputs violated the precondition"));
    if !gave_up {
        return v;
    }
    let mut ivc = crate::smt::Vc::new(vc.env());
    ivc.transparent = vc.transparent.clone();
    let inputs = ivc.inputs(decl, bin, SOLVER_INPUTS.min(cases.max(1)), seed);
    if inputs.is_empty() {
        return v;
    }
    let Some(ran) = replay_all(key, shown, decl, method, &inputs) else { return v };
    let (mut ok, mut discarded) = (0, 0);
    for r in ran {
        match r {
            Ran::Passed => ok += 1,
            Ran::Discarded => discarded += 1,
            Ran::Failed(Verdict::Counterexample { args, kind, message, .. }) => {
                return Verdict::Counterexample { args, kind, message, found_by: "fuzzer, on inputs from the solver".into(), terminates: None };
            }
            Ran::Failed(other) => return other,
        }
    }
    if ok == 0 {
        return v;
    }
    Verdict::Fuzzed { cases: ok, discarded, proven: 0, unproven: vec![], checks: vec![], open: vec![], terminates: None }
}

/// What running a function on one input showed.
enum Ran {
    Passed,
    /// Not an input it accepts: a value outside its parameter's type, or its
    /// own precondition unmet.
    Discarded,
    Failed(Verdict),
}

/// Run what `key` names on each of `inputs`, compiled as one module.
fn replay_all(key: &str, shown: &str, decl: &FnDecl, method: bool, inputs: &[Vec<Expr>]) -> Option<Vec<Ran>> {
    let prog = PROGRAM.get()?;
    let probes: Vec<Decl> = inputs.iter().enumerate().map(|(k, vals)| probe(&format!("strata_verify_replay_{k}"), replay_stmts(key, decl, method, vals))).collect();
    let faults = run_probes(prog, probes, JIT_STEPS).ok()?;
    Some(
        faults
            .into_iter()
            .zip(inputs)
            .map(|(fault, vals)| {
                let Some(fault) = fault else { return Ran::Passed };
                let stack = frames(&fault.stack);
                // an argument not of its parameter's type (a fault of the probe, which has no frame)
                if stack.is_empty() && fault.kind == "Refinement" {
                    return Ran::Discarded;
                }
                if fault.kind == "Requires" && stack.len() == 1 && fault.message.starts_with(&format!("precondition of `{key}` violated")) {
                    return Ran::Discarded;
                }
                let shown_vals: Vec<&Expr> = decl.params.iter().zip(vals).filter(|(p, _)| !(method && p.name == crate::types::type_arg("Self"))).map(|(_, v)| v).collect();
                Ran::Failed(counterexample(fault, shown_vals.iter().map(|v| crate::printer::literal(v)).collect(), key, shown))
            })
            .collect(),
    )
}

/// The statements of a probe calling what `key` names on `vals`: each bound
/// at its parameter's type, then the call.
fn replay_stmts(key: &str, decl: &FnDecl, method: bool, vals: &[Expr]) -> Vec<Stmt> {
    // a method's checked form has the hidden type argument first; its wrapper doesn't
    let params: Vec<&Param> = decl.params.iter().filter(|p| !(method && p.name == crate::types::type_arg("Self"))).collect();
    let vals: Vec<&Expr> = decl.params.iter().zip(vals).filter(|(p, _)| !(method && p.name == crate::types::type_arg("Self"))).map(|(_, v)| v).collect();
    let mut stmts = vec![];
    let mut args = vec![];
    for (i, (p, v)) in params.iter().zip(&vals).enumerate() {
        let x = (*v).clone();
        let name = Symbol::new(&format!("strata_arg_{i}"));
        // a type argument is a type's name; generic types aren't checked here
        let ty = (!matches!(&p.ty, TypeExpr::Named(n, _) if n == crate::types::TYPE_ARG)).then(|| p.ty.clone());
        if p.mutable {
            stmts.push(Stmt::Var(name, ty, x));
            args.push(Expr::Mut(Box::new(Expr::Var(name))));
        } else {
            stmts.push(Stmt::Let(Pattern::Bind(name), ty, x));
            args.push(Expr::Var(name));
        }
    }
    stmts.push(Stmt::Let(Pattern::Wild, None, Expr::Call(Box::new(Expr::Var(Symbol::new(key))), args)));
    stmts
}

/// Replace `Self` (and the impl's type parameters, at `Int`) in a method
/// signature so its inputs can be generated.
/// A type for each of an impl's type parameters that meets its bounds
/// (`Int` for `T: Zero` when `Int` implements `Zero`): what its methods are
/// checked and fuzzed at.
pub fn impl_instance(env: &Env, imp: &ImplDecl) -> crate::fx::HashMap<Symbol, TypeExpr> {
    let named = |n: &str| TypeExpr::Named(n.into(), vec![]);
    imp.type_params
        .iter()
        .map(|p| {
            let wants: Vec<Symbol> = imp.bounds.iter().filter(|b| b.param == *p).flat_map(|b| b.traits.iter().map(crate::types::texpr_head)).collect();
            let has = |h: &str| wants.iter().all(|tr| env.impls.iter().any(|i| i.trait_name == *tr && crate::types::texpr_head(&i.for_type) == h && i.type_params.is_empty()));
            // a built-in type first, then any type of the program that has them all
            let t = ["Int", "String", "Bool", "Float"]
                .iter()
                .find(|h| has(h))
                .map(|h| named(h))
                .or_else(|| env.impls.iter().filter(|i| i.type_params.is_empty()).map(|i| crate::types::texpr_head(&i.for_type)).find(|h| has(h.as_str())).map(|h| named(h.as_str())))
                .unwrap_or_else(|| named("Int"));
            (*p, t)
        })
        .collect()
}

pub fn for_impl(env: &Env, imp: &ImplDecl, f: &FnDecl) -> FnDecl {
    let mut sub = impl_instance(env, imp);
    let self_ty = crate::types::subst_texpr(&imp.for_type, &sub);
    sub.insert("Self".into(), self_ty);
    let mut g = f.clone();
    for p in &mut g.params {
        p.ty = crate::types::subst_texpr(&p.ty, &sub);
    }
    g.ret = crate::types::subst_texpr(&g.ret, &sub);
    g
}

/// An implementation's method as checked: its signature for the
/// implementation, the trait's contract (which binds every implementation)
/// before its own.
fn method_decl(env: &Env, imp: &ImplDecl, f: &FnDecl) -> FnDecl {
    let mut d = for_impl(env, imp, f);
    if let Some(tm) = env.traits.get(&imp.trait_name).and_then(|t| t.methods.iter().find(|m| m.decl.name == f.name)) {
        if !std::ptr::eq(&tm.decl, f) {
            let tm = for_impl(env, imp, &tm.decl);
            d.requires.splice(0..0, tm.requires);
            d.ensures.splice(0..0, tm.ensures);
        }
    }
    d
}

/// The wrapper of what the declaration at `index` (an implementation) has
/// under `what` (a method's name, or `law.NAME`).
pub fn wrapper_name(index: usize, what: &str) -> String {
    format!("strata_verify_{index}_{}", what.replace(|c: char| !c.is_ascii_alphanumeric(), "_"))
}

/// What `verify` fuzzes besides the program's functions, as functions too:
/// for each trait implementation chosen (declaration `i` of `prog`), a wrapper per
/// method, with its contract, calling it through the trait; and per law, one
/// whose result is the law, ensured to hold. Keyed by `wrapper_name`.
pub fn wrappers(env: &Env, prog: &Program, chosen: &[bool]) -> Vec<FnDecl> {
    let mut out = vec![];
    for (i, d) in prog.decls.iter().enumerate() {
        let DeclKind::Impl(imp) = &d.kind else { continue };
        if !chosen.get(i).copied().unwrap_or(false) {
            continue;
        }
        let head = crate::types::texpr_head(&imp.for_type);
        if imp.is_inherent() || env.capabilities.contains(&Symbol::new(&head)) {
            continue;
        }
        let Some(tr) = env.traits.get(&imp.trait_name) else { continue };
        let head = crate::types::texpr_head(&crate::types::subst_texpr(&imp.for_type, &imp.type_params.iter().map(|p| (p.clone(), TypeExpr::Named("Int".into(), vec![]))).collect()));
        for m in &tr.methods {
            let Some(decl) = imp.methods.iter().find(|x| x.name == m.decl.name).or(if m.default { Some(&m.decl) } else { None }) else { continue };
            if !decl.type_params.is_empty() || decl.params.iter().any(|p| p.comptime || matches!(p.ty, TypeExpr::Fn(..))) {
                continue;
            }
            let mut w = method_decl(env, imp, decl);
            w.name = Symbol::new(&wrapper_name(i, crate::names::short(&m.decl.name)));
            w.doc = vec![];
            w.comments = vec![];
            let call = crate::types::trait_call(&imp.trait_name, &m.decl.name);
            let vars: Vec<Expr> = w.params.iter().map(|p| if p.mutable { Expr::Mut(Box::new(Expr::Var(p.name))) } else { Expr::Var(p.name) }).collect();
            w.body = if crate::resolve::takes_self(decl) {
                let mut rest = vars;
                let recv = rest.remove(0);
                Expr::Method(Box::new(recv), Symbol::new(&call), rest)
            } else {
                let mut args = vec![Expr::Str(head.to_string())];
                args.extend(vars);
                Expr::Call(Box::new(Expr::Var(Symbol::new(&call))), args)
            };
            out.push(w);
        }
        for law in &tr.laws {
            let as_fn = FnDecl {
                doc: vec![],
                comments: vec![],
                name: Symbol::new(&wrapper_name(i, &format!("law.{}", law.name))),
                type_params: vec![],
                bounds: vec![],
                params: law.params.clone(),
                ret: TypeExpr::Named("Bool".into(), vec![]),
                requires: vec![],
                ensures: vec![Expr::Var(Symbol::new("it"))],
                terminates: false,
                body: law.body.clone(),
                comptime: false,
                derived: false,
            };
            let mut kind = DeclKind::Fn(for_impl(env, imp, &as_fn));
            // `Self.empty()` in a law means this implementation's
            let self_arg = crate::types::type_arg("Self");
            decl_exprs_mut(&mut kind, |e| {
                if matches!(e, Expr::Var(x) if x.as_str() == self_arg) {
                    *e = Expr::Str(head.to_string());
                }
            });
            let DeclKind::Fn(f) = kind else { unreachable!() };
            out.push(f);
        }
    }
    out
}

/// Fuzz an implementation's method that only the trait defines (its default).
pub fn fuzz_impl_method(imp: &ImplDecl, method: &str, key: &str, cases: usize, seed: u64) -> Verdict {
    fuzz_limited(key, &crate::types::trait_call(&imp.trait_name, method), cases, seed, None)
}

/// Check a trait law for one implementation with generated inputs: its
/// wrapper ensures it holds.
pub fn check_law(imp: &ImplDecl, law: &Law, key: &str, cases: usize, seed: u64) -> Verdict {
    let shown = format!("law `{}`", law.name);
    match fuzz_limited(key, &shown, cases, seed, None) {
        Verdict::Counterexample { args, kind, message, found_by, terminates } => {
            if kind == "Ensures" && message.contains(&format!("of {shown}")) && message.ends_with(&format!("(stack: {shown})")) || kind == "Ensures" && message.ends_with("(stack: )") {
                let with: Vec<String> = law.params.iter().zip(&args).map(|(p, v)| format!("{} = {v}", p.name)).collect();
                return Verdict::Counterexample {
                    args,
                    kind: "Law".into(),
                    message: format!("law `{}` does not hold for `{}` with {}", law.name, crate::printer::ty(&imp.for_type), with.join(", ")),
                    found_by,
                    terminates,
                };
            }
            Verdict::Counterexample { args, kind, message: format!("evaluating law `{}`: {message}", law.name), found_by, terminates }
        }
        other => other,
    }
}

/// The step budget of one cross-check run: proven code can still be too
/// costly to run (Ackermann's function), which proves nothing either way.
const CROSS_CHECK_STEPS: u64 = 200_000;

/// What is fuzzed compiled: each function's (or wrapper's) driver, those
/// that couldn't be compiled and why, and how to run a case of one (seed,
/// step budget) — the fault that ended it, if any, and its arguments.
pub struct JitFuzz {
    pub drivers: crate::fx::HashMap<String, usize>,
    pub failed: crate::fx::HashMap<String, String>,
    pub case: fn(usize, u64, i64) -> (Option<(String, String, String)>, Vec<Expr>),
    pub lock: std::sync::Mutex<()>,
}

pub static JIT_FUZZ: std::sync::OnceLock<JitFuzz> = std::sync::OnceLock::new();

/// The program verified, with the wrappers: what probes (a counterexample's
/// replay) are compiled with.
pub static PROGRAM: std::sync::OnceLock<Program> = std::sync::OnceLock::new();

/// The steps a case may take when no budget is given (each loop iteration
/// and call); past them it is a runaway case, `too many steps`.
const JIT_STEPS: i64 = 50_000_000;

/// A call stack's frames, without the wrappers' (an implementation detail).
fn frames(stack: &str) -> Vec<String> {
    if stack.is_empty() {
        return vec![];
    }
    stack.split(" -> ").filter(|s| !s.starts_with("strata_verify_")).map(|s| s.to_string()).collect()
}

/// A fault of a case as a counterexample: the wrapper's name shown as what it stands for.
fn counterexample(f: Fault, args: Vec<String>, key: &str, shown: &str) -> Verdict {
    let stack = frames(&f.stack);
    let message = f.message.replace(&format!("`{key}`"), &format!("`{shown}`"));
    Verdict::Counterexample { args, kind: f.kind, message: format!("{message} (stack: {})", short_stack(&stack)), found_by: "fuzzer".into(), terminates: None }
}

/// Fuzz what `key` names (a function or a wrapper) with its compiled driver,
/// with an optional step budget per case: with one, a case that goes past
/// it is set aside, not a failure. `shown`: what messages call it.
fn fuzz_limited(key: &str, shown: &str, cases: usize, seed: u64, budget: Option<u64>) -> Verdict {
    let b = std::sync::atomic::AtomicU64::new(budget.unwrap_or(FULL));
    fuzz_capped(key, shown, cases, seed, &b, &std::sync::atomic::AtomicUsize::new(cases))
}

/// A fuzzing budget that is `JIT_STEPS` per case, where running out of
/// steps is a failure (the code may not end); a smaller one is a cross-check's,
/// where running out only discards the case.
const FULL: u64 = u64::MAX;

/// `fuzz_limited`, stopping once `cap` cases passed, with the steps per case
/// in `budget`: both can change while it runs (the solver proved everything:
/// only a cross-check is left; it didn't: the full search). A case that ran
/// out of a cross-check's steps is run again first once the budget is full.
fn fuzz_capped(key: &str, shown: &str, cases: usize, seed: u64, budget: &std::sync::atomic::AtomicU64, cap: &std::sync::atomic::AtomicUsize) -> Verdict {
    use std::sync::atomic::Ordering::Relaxed;
    let Some(j) = JIT_FUZZ.get() else { return Verdict::Skipped { reason: "not compiled".into() } };
    let Some(&addr) = j.drivers.get(key) else {
        return Verdict::Skipped { reason: j.failed.get(key).cloned().unwrap_or_else(|| "not compiled".into()) };
    };
    let _one = j.lock.lock().unwrap_or_else(|e| e.into_inner());
    let (mut ok, mut discarded) = (0, 0);
    let (mut next, mut again): (usize, Vec<usize>) = (0, vec![]);
    loop {
        if ok >= cases.min(cap.load(Relaxed)) {
            break;
        }
        let b = budget.load(Relaxed);
        let full = b == FULL;
        let i = match (full, again.is_empty()) {
            (true, false) => again.remove(0),
            _ if next < cases * 4 => {
                next += 1;
                next - 1
            }
            _ => break,
        };
        let budget = (!full).then_some(b);
        let (fault, args) = (j.case)(addr, seed.wrapping_add(i as u64), budget.map_or(JIT_STEPS, |b| b as i64));
        let Some((kind, msg, stack)) = fault else {
            ok += 1;
            continue;
        };
        let all: Vec<&str> = if stack.is_empty() { vec![] } else { stack.split(" -> ").collect() };
        // its own precondition, unmet: not its responsibility
        if kind == "Requires" && all == [key] && msg.starts_with(&format!("precondition of `{key}` violated")) {
            discarded += 1;
            continue;
        }
        if all.is_empty() && msg.starts_with("could not generate") {
            return Verdict::Skipped { reason: msg };
        }
        if budget.is_some() && kind == "Limit" {
            // (counted as discarded only if the budget stays a cross-check's)
            again.push(i);
            continue;
        }
        if kind == "Hole" {
            return Verdict::Incomplete { message: format!("{msg} (via {})", short_stack(&frames(&stack))), terminates: None };
        }
        let args = args.iter().map(|v| crate::printer::literal(v)).collect();
        return counterexample(Fault { kind, message: msg, stack }, args, key, shown);
    }
    discarded += again.len();
    if ok == 0 && discarded > 0 {
        return Verdict::Skipped { reason: format!("all {discarded} generated inputs violated the precondition") };
    }
    Verdict::Fuzzed { cases: ok, discarded, proven: 0, unproven: vec![], checks: vec![], open: vec![], terminates: None }
}

/// A call stack for a message: deep ones (runaway recursion) are cut short.
fn short_stack(stack: &[String]) -> String {
    let stack = crate::names::fold_stack(stack);
    if stack.len() <= 8 {
        return stack.join(" -> ");
    }
    format!("{} -> … {} more … -> {}", stack[..4].join(" -> "), stack.len() - 6, stack[stack.len() - 2..].join(" -> "))
}

/// A trait method without `self` is called with the type first (`Trait::empty(type)`):
/// give its checked form that argument, so the solver knows the type.
fn static_type_arg(env: &Env, imp: &ImplDecl, f: &FnDecl, d: &mut FnDecl) {
    if !crate::resolve::takes_self(f) {
        // the whole type (`List[Int]`, not `List`): the impl's type parameters
        // are known in the method
        let whole = crate::types::subst_texpr(&imp.for_type, &impl_instance(env, imp));
        let ty = TypeExpr::Named(crate::types::TYPE_ARG.into(), vec![whole]);
        d.params.insert(0, Param { name: Symbol::from(crate::types::type_arg("Self")), ty, mutable: false, comptime: false });
    }
}
