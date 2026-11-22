//! Termination: does a function always finish?
//!
//! Shown automatically, without annotations, in the common cases:
//! * no recursion, and only `for` loops (a list is finite);
//! * a `while a < b` loop (or `<=`, `>`, `>=`) whose bound `b - a` the solver
//!   proves goes down on every pass through the body;
//! * recursion where some argument is a piece of the same parameter, taken
//!   apart by a `match` (`[x, ..rest] => f(rest)`, `Node { left, .. } => f(left)`):
//!   values are finite, so this can't go on forever;
//! * recursion with a measure: an integer expression of the parameters that
//!   the solver proves is never negative at a recursive call and smaller for
//!   its arguments (`n` in `fact(n - 1)`, `100 - n` in `up(n + 1)` under
//!   `n <= 100`, `hi - lo` in a binary search), or a pair of them compared in
//!   order (`(m, n)` for Ackermann's function).
//!
//! A function also needs everything it calls to terminate. Anything else
//! (a `loop`, other recursion, mutual recursion) "may not terminate", unless
//! it is marked `terminates`, which is reported as assumed.

use crate::ast::*;
use crate::types::{texpr_head, Env};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use crate::fx::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Termination {
    /// Shown to always finish; `how` says why.
    Proven { how: String },
    /// Finishes if the functions marked `terminates` in `by` do.
    Assumed { by: Vec<String> },
    /// Not known to finish.
    Unknown { reason: String },
}

pub struct Analysis {
    /// By function name (or `Type.method` for an implementation's method).
    pub status: HashMap<String, Termination>,
    /// Functions that call themselves, directly or through others.
    pub recursive: HashSet<String>,
    /// Functions a contract may see through (their body is unfolded): they
    /// finish and cannot fail. The flag says the finishing is only assumed.
    pub transparent: HashMap<String, bool>,
}

/// A node of the call graph: a function, an implementation's method, or a
/// trait method (which may be any implementation's).
struct Node {
    decl: Option<FnDecl>,
    calls: BTreeSet<String>,
    /// Problems with the node's own loops, if any; or how they end.
    loops: Result<Vec<String>, String>,
    /// The code whose loops `loops` is about (checked only for the nodes
    /// the analysis needs: see `analyze`).
    body: Option<FnDecl>,
}

fn trait_node(m: &str) -> String {
    // `Trait::m` (a call through one trait) reaches the implementations of `m`
    let m = crate::types::split_trait_call(m).map_or(m, |(_, m)| m);
    format!("trait method {m}")
}

/// The label of an implementation's method, as `verify` shows it.
/// When the type has a method of that name of its own, or from another
/// trait, the trait is named too: `Item.describe (Log)`.
pub fn impl_label(env: &Env, imp: &ImplDecl, m: &str) -> String {
    let short = |q: &str| crate::names::short(q).to_string();
    let head = texpr_head(&imp.for_type);
    let base = format!("{}.{}", crate::printer::ty(&imp.for_type), short(m));
    let own = env.inherent.get(&head.sym()).is_some_and(|i| i.methods.iter().any(|x| x.name == m));
    if !imp.is_inherent() && (own || env.traits_with(m, &head).len() > 1) {
        format!("{base} ({})", short(&imp.trait_name))
    } else {
        base
    }
}

/// The functions and trait methods `e` calls or passes as values. `locals`
/// are names bound outside `e` (the parameters): a local of the same name as
/// a function hides it (`mut app := Router.new()` inside `fn app`).
fn calls_of(e: &Expr, env: &Env, locals: &[String], out: &mut BTreeSet<String>) {
    calls_within(e, env, locals, None, out)
}

/// `calls_of` in an implementation of trait method `own` (its trait node):
/// a call of `own` on a field of `self` (`self.inner.len()` in `len`) is
/// left out. It is recursion on a strictly smaller value (values are finite
/// trees), whichever implementation it reaches; an implementation that
/// calls `own` on anything else is a cycle through the trait method still.
fn calls_within(e: &Expr, env: &Env, locals: &[String], own: Option<&str>, out: &mut BTreeSet<String>) {
    use crate::ast::{walk_expr, walk_pattern, Visit};
    /// The names a pattern binds.
    struct Binds(Vec<String>);
    impl Visit for Binds {
        fn pattern(&mut self, p: &Pattern) {
            if let Pattern::Bind(n) = p {
                self.0.push(n.to_string().to_string());
            }
            walk_pattern(self, p)
        }
        fn expr(&mut self, _: &Expr) {}
    }
    fn binds(p: &Pattern) -> Vec<String> {
        let mut b = Binds(vec![]);
        b.pattern(p);
        b.0
    }
    struct Calls<'a> {
        env: &'a Env,
        out: &'a mut BTreeSet<String>,
        scopes: Vec<Vec<String>>,
        own: Option<&'a str>,
    }
    /// `self.a.b`: a field (of a field) of `self`.
    fn self_field(e: &Expr) -> bool {
        match e {
            Expr::Field(x, _) => matches!(&**x, Expr::Var(v) if v.as_str() == "self") || self_field(x),
            _ => false,
        }
    }
    impl Calls<'_> {
        fn local(&self, n: &str) -> bool {
            self.scopes.iter().any(|s| s.iter().any(|x| x == n))
        }
        fn name(&mut self, n: &str) {
            if self.local(n) {
                return;
            }
            if self.env.fns.contains_key(&Symbol::new(n)) {
                self.out.insert(n.to_string());
            } else if self.env.is_method(n.sym()) {
                self.out.insert(trait_node(n));
            }
        }
        fn scoped(&mut self, names: Vec<String>, e: &Expr) {
            self.scopes.push(names);
            self.expr(e);
            self.scopes.pop();
        }
    }
    impl Visit for Calls<'_> {
        fn expr(&mut self, e: &Expr) {
            match e {
                // a function called, or passed as a value (it may be called)
                Expr::Var(n) => self.name(n),
                Expr::Method(r, m, args) => {
                    // `next` on the `FiniteIterator` a `for` runs over (types::lower_iterator_fors):
                    // the iterator's `len` says it returns, as many times as the loop runs
                    let finite = matches!(&**r, Expr::Var(v) if v.as_str().starts_with(crate::types::FINITE_ITER_VAR));
                    let smaller = self.own.is_some_and(|o| trait_node(m) == o) && self_field(r);
                    if !finite && !smaller {
                        self.name(m);
                    }
                    self.expr(r);
                    args.iter().for_each(|a| self.expr(a));
                }
                Expr::Lambda(ps, body) => self.scoped(ps.iter().map(|p| p.name.to_string()).collect(), body),
                Expr::Match(scrut, _, arms) => {
                    self.expr(scrut);
                    for a in arms {
                        self.pattern(&a.pat);
                        self.scoped(binds(&a.pat), &a.body);
                    }
                }
                Expr::Block(stmts, last) => {
                    // each binding is visible from the next statement on
                    self.scopes.push(vec![]);
                    for st in stmts {
                        match st {
                            Stmt::ComptimeFor(..) => {}
                            Stmt::Let(p, _, x) => {
                                self.expr(x);
                                self.pattern(p);
                                let b = binds(p);
                                self.scopes.last_mut().unwrap().extend(b);
                            }
                            Stmt::Var(n, _, x) => {
                                self.expr(x);
                                self.scopes.last_mut().unwrap().push(n.to_string().to_string());
                            }
                            Stmt::For(p, xs, body) => {
                                self.expr(xs);
                                self.pattern(p);
                                self.scoped(binds(p), body);
                            }
                            Stmt::Assign(a, b) | Stmt::While(a, b) => {
                                self.expr(a);
                                self.expr(b);
                            }
                            Stmt::Loop(x) | Stmt::Expr(x) | Stmt::Assert(x) | Stmt::Invariant(x) => self.expr(x),
                            Stmt::Comment(_) | Stmt::Break | Stmt::Continue => {}
                        }
                    }
                    self.expr(last);
                    self.scopes.pop();
                }
                _ => walk_expr(self, e),
            }
        }
        // conditions in patterns (`where …`) may call functions too
        fn pattern(&mut self, p: &Pattern) {
            walk_pattern(self, p)
        }
        fn ty(&mut self, _: &TypeExpr) {}
    }
    Calls { env, out, scopes: vec![locals.to_vec()], own }.expr(e)
}

fn has_stmt(e: &Expr, pred: &dyn Fn(&Stmt) -> bool) -> bool {
    let mut found = false;
    crate::ast::walk(e, |x| {
        if let Expr::Block(stmts, _) = x {
            found |= stmts.iter().any(pred);
        }
    });
    found
}

/// Whether `e` has a `loop` other than one running over a `FiniteIterator`
/// (`types::lower_iterator_fors`: the variable holding it just before),
/// which ends once `next` has given the iterator's `len` elements.
fn any_open_loop(e: &Expr) -> bool {
    let mut found = false;
    crate::ast::walk(e, |x| {
        if let Expr::Block(stmts, _) = x {
            for (i, s) in stmts.iter().enumerate() {
                let finite = i > 0 && matches!(&stmts[i - 1], Stmt::Var(v, _, _) if v.as_str().starts_with(crate::types::FINITE_ITER_VAR));
                found |= matches!(s, Stmt::Loop(_)) && !finite;
            }
        }
    });
    found
}

/// The loops of a body: `Ok(how they end)` or `Err(why one may not)`.
fn loops(env: &Env, f: &FnDecl, solver: Option<&crate::smt::Solver>) -> Result<Vec<String>, String> {
    // a `for` over an `Iterator` is a `loop` calling `next` (types::lower_iterator_fors)
    let over_iterator = |s: &Stmt| matches!(s, Stmt::Var(v, _, _) if v.as_str().starts_with(crate::types::ITER_VAR));
    if has_stmt(&f.body, &over_iterator) {
        return Err("it has a `for` over an `Iterator` that isn't a `FiniteIterator`, which may never end (only `for` loops over lists, sets, dicts and finite iterators, and `while` loops whose bound goes down, are shown to end)".into());
    }
    if any_open_loop(&f.body) {
        return Err("it has a `loop` (only `for` loops, and `while` loops whose bound goes down, are shown to end)".into());
    }
    if !has_stmt(&f.body, &|s| matches!(s, Stmt::While(..))) {
        return Ok(vec![]);
    }
    let Some(bin) = solver else { return Err("it has a `while` loop, and no SMT solver was found to check its bound".into()) };
    let mut vc = crate::smt::Vc::new(env);
    // (the loops' invariants help show their bounds go down)
    vc.solver = Some(bin.clone());
    vc.function(f, &f.name);
    let mut hows = vec![];
    for lp in vc.loops.clone() {
        if let Some(why) = &lp.reason {
            return Err(why.clone());
        }
        if lp.always_exits {
            hows.push(format!("`{}` leaves the loop on every pass", lp.what));
            continue;
        }
        let ok = lp.candidates.iter().find(|(_, i)| matches!(vc.solve(bin, *i), crate::smt::Outcome::Proven));
        match ok {
            Some((m, _)) => hows.push(format!("`{}` ends: `{m}` goes down on every pass", lp.what)),
            None if lp.candidates.is_empty() => return Err(format!("the loop `{}` has no bound to check (its condition is not a comparison)", lp.what)),
            None => return Err(format!("the loop `{}`: no bound was shown to go down (tried {})", lp.what, lp.candidates.iter().map(|c| format!("`{}`", c.0)).collect::<Vec<_>>().join(", "))),
        }
    }
    Ok(hows)
}

// ---- structural recursion ----

/// For a variable: the parameters it is a piece of (or equal to), and those
/// it is a strictly smaller piece of.
#[derive(Clone, Default)]
struct Piece {
    within: BTreeSet<usize>,
    smaller: BTreeSet<usize>,
}

struct Structural<'a> {
    name: &'a str,
    assigned: HashSet<String>,
    /// For each recursive call: the parameters whose argument is smaller.
    calls: Vec<BTreeSet<usize>>,
}

impl Structural<'_> {
    fn piece(&self, e: &Expr, vars: &HashMap<String, Piece>) -> Piece {
        match e {
            Expr::Var(x) => vars.get(x.as_str()).cloned().unwrap_or_default(),
            Expr::Field(r, _) => {
                let p = self.piece(r, vars);
                Piece { smaller: p.within.clone(), within: p.within }
            }
            _ => Piece::default(),
        }
    }

    /// Bind the variables of `p` matched against `scrut`. `depth` > 0 means
    /// inside a constructor or list: a strictly smaller piece.
    fn bind(&self, p: &Pattern, scrut: Option<&Expr>, whole: &Piece, depth: usize, vars: &mut HashMap<String, Piece>) {
        match p {
            Pattern::For(..) => {}
            Pattern::Bind(x) => {
                let piece = if self.assigned.contains(x.as_str()) {
                    Piece::default()
                } else if depth > 0 {
                    Piece { within: whole.within.clone(), smaller: whole.within.clone() }
                } else {
                    whole.clone()
                };
                vars.insert(x.to_string(), piece);
            }
            Pattern::Tuple(ps) => match scrut {
                Some(Expr::Tuple(es)) if es.len() == ps.len() && depth == 0 => {
                    for (p, e) in ps.iter().zip(es) {
                        let w = self.piece(e, vars);
                        self.bind(p, Some(e), &w, 0, vars);
                    }
                }
                _ => ps.iter().for_each(|p| self.bind(p, None, whole, depth, vars)),
            },
            Pattern::List(ps, rest) => {
                for p in ps {
                    self.bind(p, None, whole, depth + 1, vars);
                }
                if let Some(r) = rest {
                    self.bind(&Pattern::Bind(r.clone()), None, whole, depth + 1, vars);
                }
            }
            Pattern::Ctor(_, cp) => match cp {
                CtorPat::None => {}
                CtorPat::Positional(ps) | CtorPat::Type(ps) => ps.iter().for_each(|p| self.bind(p, None, whole, depth + 1, vars)),
                CtorPat::Record(fs, _) => fs.iter().for_each(|(_, p)| self.bind(p, None, whole, depth + 1, vars)),
            },
            Pattern::Or(alts) => {
                // every alternative binds the same names; keep what holds in all of them
                let mut all: Option<HashMap<String, Piece>> = None;
                for a in alts {
                    let mut v = vars.clone();
                    self.bind(a, scrut, whole, depth, &mut v);
                    all = Some(match all {
                        None => v,
                        Some(prev) => prev
                            .into_iter()
                            .map(|(k, p)| {
                                let q = v.get(&k).cloned().unwrap_or_default();
                                (k, Piece { within: p.within.intersection(&q.within).cloned().collect(), smaller: p.smaller.intersection(&q.smaller).cloned().collect() })
                            })
                            .collect(),
                    });
                }
                if let Some(a) = all {
                    *vars = a;
                }
            }
            Pattern::Wild | Pattern::Int(_) | Pattern::Str(_) | Pattern::Bool(_) | Pattern::Cond(_) => {}
        }
    }

    fn pattern_conds(&mut self, p: &Pattern, vars: &HashMap<String, Piece>) {
        match p {
            Pattern::Cond(c) => {
                let mut v = vars.clone();
                v.remove("it");
                self.expr(c, &v);
            }
            Pattern::Or(ps) | Pattern::Tuple(ps) => ps.iter().for_each(|p| self.pattern_conds(p, vars)),
            Pattern::List(ps, _) => ps.iter().for_each(|p| self.pattern_conds(p, vars)),
            Pattern::Ctor(_, CtorPat::Positional(ps)) => ps.iter().for_each(|p| self.pattern_conds(p, vars)),
            Pattern::Ctor(_, CtorPat::Record(fs, _)) => fs.iter().for_each(|(_, p)| self.pattern_conds(p, vars)),
            _ => {}
        }
    }

    fn call(&mut self, args: &[Expr], vars: &HashMap<String, Piece>) {
        let smaller: BTreeSet<usize> = args.iter().enumerate().filter(|(i, a)| self.piece(a, vars).smaller.contains(i)).map(|(i, _)| i).collect();
        self.calls.push(smaller);
    }

    fn expr(&mut self, e: &Expr, vars: &HashMap<String, Piece>) {
        match e {
            Expr::Call(f, args) => {
                if matches!(&**f, Expr::Var(n) if n == self.name) {
                    self.call(args, vars);
                }
                self.expr(f, vars);
                args.iter().for_each(|a| self.expr(a, vars));
            }
            Expr::Method(r, m, args) => {
                if m == self.name {
                    let mut all = vec![(**r).clone()];
                    all.extend(args.iter().cloned());
                    self.call(&all, vars);
                }
                self.expr(r, vars);
                args.iter().for_each(|a| self.expr(a, vars));
            }
            Expr::Match(s, name, arms) => {
                self.expr(s, vars);
                let whole = self.piece(s, vars);
                for a in arms {
                    let mut v = vars.clone();
                    // a recursive call in a `where` pattern counts too
                    self.pattern_conds(&a.pat, vars);
                    if let Some(n) = name {
                        v.insert(n.to_string(), whole.clone());
                    }
                    self.bind(&a.pat, Some(s), &whole, 0, &mut v);
                    self.expr(&a.body, &v);
                }
            }
            Expr::Lambda(ps, body) => {
                let mut v = vars.clone();
                for p in ps {
                    v.remove(p.name.as_str());
                }
                self.expr(body, &v);
            }
            Expr::Block(stmts, last) => {
                let mut v = vars.clone();
                for s in stmts {
                    match s {
                        Stmt::ComptimeFor(..) => {}
                        Stmt::Let(p, _, x) => {
                            self.expr(x, &v);
                            let w = self.piece(x, &v);
                            self.bind(p, Some(x), &w, 0, &mut v);
                        }
                        Stmt::Var(x, _, init) => {
                            self.expr(init, &v);
                            v.remove(x.as_str());
                        }
                        Stmt::For(p, xs, body) => {
                            self.expr(xs, &v);
                            let w = self.piece(xs, &v);
                            let mut v2 = v.clone();
                            // an element is a smaller piece of the list
                            self.bind(p, None, &w, 1, &mut v2);
                            self.expr(body, &v2);
                        }
                        Stmt::Assign(a, b) | Stmt::While(a, b) => {
                            self.expr(a, &v);
                            self.expr(b, &v);
                        }
                        Stmt::Expr(x) | Stmt::Assert(x) | Stmt::Loop(x) | Stmt::Invariant(x) => self.expr(x, &v),
                        Stmt::Break | Stmt::Continue | Stmt::Comment(_) => {}
                    }
                }
                self.expr(last, &v);
            }
            other => {
                // every other expression: just visit the children
                let mut kids: Vec<Expr> = vec![];
                match other {
                    Expr::List(xs) | Expr::Tuple(xs) => kids.extend(xs.iter().cloned()),
                    Expr::Ctor(_, CtorArgs::Positional(xs)) => kids.extend(xs.iter().cloned()),
                    Expr::Ctor(_, CtorArgs::Record(fs)) => kids.extend(fs.iter().map(|f| f.1.clone())),
                    Expr::Ctor(_, CtorArgs::Update(b, fs)) => {
                        kids.push((**b).clone());
                        kids.extend(fs.iter().map(|f| f.1.clone()));
                    }
                    Expr::Field(r, _) | Expr::Is(r, _) | Expr::Un(_, r) | Expr::Return(r) | Expr::Try(r, _) | Expr::Mut(r) => kids.push((**r).clone()),
                    Expr::Interp(parts) => kids.extend(parts.iter().filter_map(|p| if let StrPart::Expr(x, _) = p { Some(x.clone()) } else { None })),
                    Expr::Or(x, fb) => {
                        self.expr(x, vars);
                        self.expr(fb, vars);
                    }
                    Expr::Except(x, name, fb) => {
                        self.expr(x, vars);
                        let mut v = vars.clone();
                        if let Some(n) = name {
                            v.remove(n.as_str());
                        }
                        self.expr(fb, &v);
                    }
                    Expr::Bin(_, l, r) => kids.extend([(**l).clone(), (**r).clone()]),
                    Expr::If(c, t, f) => kids.extend([(**c).clone(), (**t).clone(), (**f).clone()]),
                    _ => {}
                }
                kids.iter().for_each(|k| self.expr(k, vars));
            }
        }
    }
}

fn assigned(env: &Env, e: &Expr) -> HashSet<String> {
    changed_vars(e, &|n| env.mut_params(n))
}

/// If every recursive call passes a smaller piece of the same parameter,
/// the name of that parameter.
fn structural(env: &Env, f: &FnDecl) -> Option<String> {
    let assigned = assigned(env, &f.body);
    let mut vars = HashMap::default();
    for (i, p) in f.params.iter().enumerate() {
        if !assigned.contains(p.name.as_str()) && !p.mutable {
            vars.insert(p.name.to_string(), Piece { within: [i].into(), smaller: BTreeSet::new() });
        }
    }
    let mut s = Structural { name: &f.name, assigned, calls: vec![] };
    s.expr(&f.body, &vars);
    let mut common: Option<BTreeSet<usize>> = None;
    for c in s.calls {
        common = Some(match common {
            None => c,
            Some(prev) => prev.intersection(&c).cloned().collect(),
        });
    }
    common.and_then(|c| c.into_iter().next()).map(|i| f.params[i].name.to_string())
}

/// Candidate measures of a recursive function: integer expressions of its
/// parameters that might go down on every recursive call. Each integer
/// parameter, the length of each list or string parameter, and, for every
/// comparison of parameters in the body (`n <= 100`, `lo < hi`), the distance
/// between its two sides, both ways (`100 - n`, `hi - lo`).
fn candidate_measures(env: &Env, f: &FnDecl) -> Vec<Expr> {
    let assigned = assigned(env, &f.body);
    let params: HashSet<&str> = f.params.iter().filter(|p| !p.mutable && !assigned.contains(p.name.as_str())).map(|p| p.name.as_str()).collect();
    let var = |n: &str| Expr::Var(Symbol::from(n));
    let mut out: Vec<Expr> = vec![];
    for p in &f.params {
        if !params.contains(p.name.as_str()) {
            continue;
        }
        match env.expand_alias(&p.ty) {
            TypeExpr::Named(n, _) if n == "Int" => out.push(var(&p.name)),
            TypeExpr::Named(n, _) if ["List", "String", "Dict", "Set"].contains(&n.as_str()) => out.push(Expr::Call(Box::new(var("len")), vec![var(&p.name)])),
            _ => {}
        }
    }
    // only expressions of the parameters (and functions) can be measures
    let only_params = |e: &Expr| {
        let mut ok = true;
        crate::ast::walk(e, |x| match x {
            Expr::Var(n) => ok &= params.contains(n.as_str()) || env.fns.contains_key(&n.sym()) || crate::types::BUILTINS.iter().any(|b| b.name == n),
            Expr::Lambda(..) | Expr::Hole(_) | Expr::Float(_) | Expr::Str(_) => ok = false,
            _ => {}
        });
        ok
    };
    crate::ast::walk(&f.body, |x| {
        if let Expr::Bin(BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Ne | BinOp::Eq, a, b) = x {
            if only_params(a) && only_params(b) {
                let sub = |l: &Expr, r: &Expr| Expr::Bin(BinOp::Sub, Box::new(l.clone()), Box::new(r.clone()));
                out.push(sub(b, a));
                out.push(sub(a, b));
            }
        }
    });
    let mut seen = HashSet::default();
    out.retain(|m| seen.insert(crate::printer::expr(m, 0)));
    out.truncate(8);
    out
}

/// Find a measure that goes down on every recursive call and is never
/// negative there: one candidate, or else a pair compared in order (as for
/// Ackermann's function). Checked by the solver; returns the measure, shown.
fn measured_recursion(env: &Env, f: &FnDecl, solver: Option<&crate::smt::Solver>) -> Option<String> {
    let bin = solver?;
    let mut vc = crate::smt::Vc::new(env);
    vc.function(f, &f.name);
    let ms = find_measure(env, f, &mut vc, bin)?;
    let shown: Vec<String> = ms.iter().map(|m| crate::printer::expr(m, 0)).collect();
    Some(if shown.len() == 1 { format!("`{}`", shown[0]) } else { format!("the pair ({}), compared in order,", shown.iter().map(|s| format!("`{s}`")).collect::<Vec<_>>().join(", ")) })
}

/// The first candidate measure (or pair) that goes down at every recursive call.
fn find_measure(env: &Env, f: &FnDecl, vc: &mut crate::smt::Vc, bin: &crate::smt::Solver) -> Option<Vec<Expr>> {
    if vc.rec_sites.is_empty() {
        return None;
    }
    let cands = candidate_measures(env, f);
    let mut tries: Vec<Vec<Expr>> = cands.iter().map(|c| vec![c.clone()]).collect();
    for a in &cands {
        for b in &cands {
            if crate::printer::expr(a, 0) != crate::printer::expr(b, 0) {
                tries.push(vec![a.clone(), b.clone()]);
            }
        }
    }
    tries.into_iter().find(|ms| {
        (0..vc.rec_sites.len()).all(|s| {
            let k = vc.measure_check(s, ms);
            matches!(vc.solve(bin, k), crate::smt::Outcome::Proven)
        })
    })
}

/// The solver queries behind a function's termination (for `verify --smt`):
/// each `while` loop's candidate bounds, and for recursion, the measure that
/// was found at each recursive call (or, if none was, each single candidate).
pub fn queries(env: &Env, f: &FnDecl, solver: Option<&crate::smt::Solver>) -> Vec<(String, String)> {
    let mut vc = crate::smt::Vc::new(env);
    vc.function(f, &f.name);
    let mut out: Vec<(String, String)> = vec![];
    for i in 0..vc.obligations.len() {
        if vc.obligations[i].termination {
            out.push((format!("termination: {}", vc.obligations[i].what), vc.smtlib(i)));
        }
    }
    if vc.rec_sites.is_empty() {
        return out;
    }
    let (found, tried) = match solver.and_then(|bin| find_measure(env, f, &mut vc, bin)) {
        Some(ms) => (true, vec![ms]),
        None => (false, candidate_measures(env, f).into_iter().map(|m| vec![m]).collect()),
    };
    for ms in tried {
        for s in 0..vc.rec_sites.len() {
            let k = vc.measure_check(s, &ms);
            let note = if found { "" } else { " (tried; no measure was found)" };
            out.push((format!("termination: {} at recursive call {}{note}", vc.obligations[k].what, s + 1), vc.smtlib(k)));
        }
    }
    out
}

// ---- transparency ----

/// Can evaluating `f`'s body fail (a runtime check, a hole)? Calls to other
/// functions are fine when those cannot fail either.
fn cannot_fail(env: &Env, f: &FnDecl, total: &dyn Fn(&str) -> bool) -> bool {
    if !f.requires.is_empty() || f.params.iter().any(|p| p.mutable) {
        return false;
    }
    let mut ok = true;
    let refined = |t: &str| env.types.get(&t.sym()).is_some_and(|_| crate::smt::type_is_refined(env, &TypeExpr::Named(t.into(), vec![])));
    crate::ast::walk(&f.body, |x| match x {
        // an early return would need its own unfolding
        Expr::Hole(_) | Expr::Mock(_) | Expr::Return(_) | Expr::Try(..) => ok = false,
        Expr::Bin(BinOp::Div | BinOp::Mod, _, d) => ok &= matches!(&**d, Expr::Int(n) if *n != 0),
        Expr::Ctor(c, _) => ok &= !refined(&env.ctors.get(&c.sym()).cloned().unwrap_or_default()),
        Expr::Call(g, _) => {
            if let Expr::Var(n) = &**g {
                ok &= if env.fns.contains_key(&n.sym()) { total(n) } else { !env.is_method(n.sym()) && crate::types::total_builtin(n) };
            }
        }
        Expr::Method(_, n, _) => ok &= if env.fns.contains_key(&n.sym()) { total(n) } else { !env.is_method(n.sym()) && crate::types::total_builtin(n) },
        Expr::Block(stmts, _) => ok &= !stmts.iter().any(|s| matches!(s, Stmt::Assert(_))),
        _ => {}
    });
    ok
}

/// The predicates of the refinements written in `t`.
fn refinements_in(t: &TypeExpr, out: &mut Vec<Expr>) {
    match t {
        TypeExpr::Refined(b, p) => {
            out.push((**p).clone());
            refinements_in(b, out);
        }
        TypeExpr::Named(_, args) => args.iter().for_each(|a| refinements_in(a, out)),
        TypeExpr::Union(ms) => ms.iter().for_each(|m| refinements_in(m, out)),
        TypeExpr::Fn(ps, r) => {
            ps.iter().for_each(|p| refinements_in(p, out));
            refinements_in(r, out);
        }
        TypeExpr::Unit => {}
    }
}

/// The termination verdict of every function and implementation method
/// (`roots: None`), or of those `roots` reach: a verdict depends only on what
/// the code calls, and showing that loops end costs a solver run per loop, so
/// verifying one function does not pay for the whole program's (std's) loops.
/// The others are left unknown, and none of them is transparent.
/// The solver's findings about std's own functions (a loop's bound, a
/// recursion's measure), kept between runs: they depend only on std, which
/// is part of the strata binary, and on the solver. One file per binary and
/// solver, under `~/.cache/strata/termination` (`STRATA_CACHE` moves it).
/// Functions made from std for a program (`std.json.encode_as.for.A.2`) are
/// not std's own, and are not kept.
struct StdCache {
    path: Option<std::path::PathBuf>,
    loops: HashMap<String, Result<Vec<String>, String>>,
    measures: HashMap<String, Option<String>>,
    changed: bool,
}

impl StdCache {
    fn load(solver: Option<&crate::smt::Solver>) -> StdCache {
        let path = solver.and_then(Self::file);
        let mut c = StdCache { path, loops: HashMap::default(), measures: HashMap::default(), changed: false };
        let text = c.path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        for line in text.lines() {
            let f: Vec<String> = line.split('\t').map(unescape).collect();
            match f.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
                ["L", name, "ok", rest @ ..] => drop(c.loops.insert(name.to_string(), Ok(rest.iter().map(|s| s.to_string()).collect()))),
                ["L", name, "err", why] => drop(c.loops.insert(name.to_string(), Err(why.to_string()))),
                ["M", name, "some", m] => drop(c.measures.insert(name.to_string(), Some(m.to_string()))),
                ["M", name, "none"] => drop(c.measures.insert(name.to_string(), None)),
                _ => {}
            }
        }
        c
    }

    fn file(solver: &crate::smt::Solver) -> Option<std::path::PathBuf> {
        use sha2::{Digest, Sha256};
        let exe = std::env::current_exe().ok()?;
        let meta = std::fs::metadata(&exe).ok()?;
        let key: String = Sha256::digest(format!("termination v1 {:?} {} {} {}", meta.modified().ok()?, meta.len(), solver.bin, solver.version).as_bytes()).iter().take(16).map(|b| format!("{b:02x}")).collect();
        Some(crate::verify::cache_root()?.join("termination").join(key))
    }

    /// Is `name` one of std's own functions?
    fn std_own(name: &str) -> bool {
        static NAMES: std::sync::OnceLock<HashSet<String>> = std::sync::OnceLock::new();
        NAMES.get_or_init(|| crate::stdlib::program().decls.iter().map(|d| d.qname()).collect()).contains(name)
    }

    fn put_loops(&mut self, name: &str, r: &Result<Vec<String>, String>) {
        if self.path.is_some() && Self::std_own(name) {
            self.loops.insert(name.to_string(), r.clone());
            self.changed = true;
        }
    }

    fn put_measure(&mut self, name: &str, m: &Option<String>) {
        if self.path.is_some() && Self::std_own(name) {
            self.measures.insert(name.to_string(), m.clone());
            self.changed = true;
        }
    }

    /// Written whole, then renamed into place (another strata may be writing too).
    fn save(&self) {
        let (Some(path), true) = (&self.path, self.changed) else { return };
        let mut lines: Vec<String> = vec![];
        for (n, r) in &self.loops {
            let mut f = vec!["L".to_string(), escape(n)];
            match r {
                Ok(hows) => {
                    f.push("ok".into());
                    f.extend(hows.iter().map(|h| escape(h)));
                }
                Err(why) => f.extend(["err".to_string(), escape(why)]),
            }
            lines.push(f.join("\t"));
        }
        for (n, m) in &self.measures {
            lines.push(match m {
                Some(m) => format!("M\t{}\tsome\t{}", escape(n), escape(m)),
                None => format!("M\t{}\tnone", escape(n)),
            });
        }
        lines.sort();
        let Some(dir) = path.parent() else { return };
        let tmp = dir.join(format!(".{}.{}", std::process::id(), path.file_name().and_then(|f| f.to_str()).unwrap_or("t")));
        let _ = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&tmp, lines.join("\n") + "\n")).and_then(|_| std::fs::rename(&tmp, path));
    }
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

pub fn analyze(env: &Env, solver: Option<&crate::smt::Solver>, roots: Option<&HashSet<String>>) -> Analysis {
    let mut nodes: HashMap<String, Node> = HashMap::default();
    for (name, f) in &env.fns {
        let mut calls = BTreeSet::new();
        let params: Vec<String> = f.params.iter().map(|p| p.name.to_string()).collect();
        calls_of(&f.body, env, &params, &mut calls);
        f.requires.iter().chain(&f.ensures).for_each(|c| calls_of(c, env, &params, &mut calls));
        nodes.insert(name.to_string().to_string(), Node { decl: Some(f.clone()), calls, loops: Ok(vec![]), body: Some(f.clone()) });
    }
    for imp in &env.impls {
        let Some(tr) = env.traits.get(&imp.trait_name) else { continue };
        for tm in &tr.methods {
            let decl = imp.methods.iter().find(|m| m.name == tm.decl.name).or(tm.default.then_some(&tm.decl));
            let Some(decl) = decl else { continue };
            let label = impl_label(env, imp, &tm.decl.name);
            let mut calls = BTreeSet::new();
            let params: Vec<String> = decl.params.iter().map(|p| p.name.to_string()).collect();
            calls_within(&decl.body, env, &params, Some(&trait_node(&tm.decl.name)), &mut calls);
            let d = crate::verify::for_impl(env, imp, decl);
            nodes.insert(label.clone(), Node { decl: None, calls, loops: Ok(vec![]), body: Some(d) });
            nodes.entry(trait_node(&tm.decl.name)).or_insert(Node { decl: None, calls: BTreeSet::new(), loops: Ok(vec![]), body: None }).calls.insert(label);
        }
        let _ = texpr_head(&imp.for_type);
    }
    // (a type's own methods are functions, above)
    // trait methods no implementation defines
    let missing: Vec<String> = nodes.values().flat_map(|n| n.calls.iter().cloned()).filter(|c| !nodes.contains_key(c)).collect();
    for m in missing {
        nodes.insert(m, Node { decl: None, calls: BTreeSet::new(), loops: Ok(vec![]), body: None });
    }

    // what each node reaches
    let reach = |start: &str| -> HashSet<String> {
        let mut seen = HashSet::default();
        let mut stack: Vec<String> = nodes[start].calls.iter().cloned().collect();
        while let Some(n) = stack.pop() {
            if seen.insert(n.clone()) {
                stack.extend(nodes[&n].calls.iter().cloned());
            }
        }
        seen
    };
    let reaches: HashMap<String, HashSet<String>> = nodes.keys().map(|k| (k.clone(), reach(k))).collect();
    let recursive: HashSet<String> = nodes.keys().filter(|n| reaches[n.as_str()].contains(n.as_str())).cloned().collect();
    // the nodes whose verdict is wanted: the roots, the helpers refinements
    // call (seen through wherever a type is checked), and what they reach
    let roots: Option<HashSet<String>> = roots.map(|rs| {
        let mut rs = rs.clone();
        let mut preds: Vec<Expr> = vec![];
        for td in env.types.values() {
            preds.extend(td.refinement.iter().cloned());
            match &td.body {
                TypeBody::Alias(a) => refinements_in(a, &mut preds),
                TypeBody::Record(fs) => fs.iter().for_each(|f| refinements_in(&f.ty, &mut preds)),
                TypeBody::Sum(vs) => {
                    for v in vs {
                        match &v.payload {
                            Payload::Positional(ts) => ts.iter().for_each(|t| refinements_in(t, &mut preds)),
                            Payload::Record(fs) => fs.iter().for_each(|f| refinements_in(&f.ty, &mut preds)),
                            Payload::Type(t) => refinements_in(t, &mut preds),
                            Payload::None => {}
                        }
                    }
                }
            }
        }
        for f in env.fns.values() {
            f.params.iter().for_each(|p| refinements_in(&p.ty, &mut preds));
            refinements_in(&f.ret, &mut preds);
        }
        let mut calls = BTreeSet::new();
        for e in &preds {
            calls_of(e, env, &["it".to_string()], &mut calls);
        }
        rs.extend(calls);
        rs
    });
    let needed: Option<HashSet<String>> = roots.as_ref().map(|rs| rs.iter().filter(|r| nodes.contains_key(r.as_str())).flat_map(|r| reaches[r.as_str()].iter().cloned().chain([r.clone()])).collect());
    let wanted = |n: &str| needed.as_ref().is_none_or(|s| s.contains(n));
    let skipped = "not analysed (nothing being verified reaches it)".to_string();
    // the loops' bounds of what is wanted, each function's on a thread (each
    // query a solver process)
    // (std's are kept between runs: `StdCache`)
    let mut cache = StdCache::load(solver);
    let todo: Vec<(String, FnDecl)> = nodes.iter().filter_map(|(name, n)| n.body.clone().filter(|_| wanted(name) && !cache.loops.contains_key(name)).map(|b| (name.clone(), b))).collect();
    let mut found: HashMap<String, Result<Vec<String>, String>> = crate::par::map(&todo, |(name, b)| (name.clone(), loops(env, b, solver))).into_iter().collect();
    for (name, r) in &found {
        cache.put_loops(name, r);
    }
    found.extend(cache.loops.iter().filter(|(n, _)| nodes.contains_key(n.as_str())).map(|(n, r)| (n.clone(), r.clone())));
    for (name, n) in nodes.iter_mut() {
        if n.body.is_some() {
            n.loops = found.get(name).cloned().unwrap_or_else(|| Err(skipped.clone()));
        }
    }
    let mutual = |n: &str| -> Option<String> { nodes[n].calls.iter().find(|c| c.as_str() != n && reaches[c.as_str()].contains(n)).cloned() };

    // each node's own verdict, ignoring what it calls
    // the measures of the functions that call themselves on no smaller piece
    // of a parameter, each on a thread (each a few solver processes)
    let needs: Vec<(&String, &FnDecl)> = nodes
        .iter()
        .filter(|(name, n)| wanted(name) && n.loops.is_ok() && mutual(name).is_none() && n.calls.contains(*name) && !cache.measures.contains_key(name.as_str()))
        .filter_map(|(name, n)| n.decl.as_ref().filter(|f| structural(env, f).is_none()).map(|f| (name, f)))
        .collect();
    let mut measures: HashMap<String, Option<String>> = crate::par::map(&needs, |(name, f)| ((*name).clone(), measured_recursion(env, f, solver))).into_iter().collect();
    for (name, m) in &measures {
        cache.put_measure(name, m);
    }
    measures.extend(cache.measures.clone());
    cache.save();
    let mut own: HashMap<String, Result<String, String>> = HashMap::default();
    for (name, n) in &nodes {
        if !wanted(name) {
            own.insert(name.clone(), Err(skipped.clone()));
            continue;
        }
        let r = match &n.loops {
            Err(e) => Err(e.clone()),
            Ok(hows) => {
                if let Some(other) = mutual(name) {
                    // a trait method by its short name (`next`, not `std.prelude.next`)
                    let shown = other.strip_prefix("trait method ").map_or(other.as_str(), crate::names::short);
                    Err(format!("it is recursive through `{shown}`"))
                } else if n.calls.contains(name) {
                    match n.decl.as_ref().and_then(|f| structural(env, f)) {
                        Some(p) => Ok(format!("each recursive call gets a smaller piece of `{p}`")),
                        None => match measures.get(name).cloned().unwrap_or_else(|| n.decl.as_ref().and_then(|f| measured_recursion(env, f, solver))) {
                            Some(m) => Ok(format!("{m} goes down on every recursive call and is never negative there")),
                            None => Err("it calls itself, and nothing was found that gets smaller on every call (a piece of a parameter, or a non-negative measure of the parameters)".into()),
                        },
                    }
                } else if hows.is_empty() {
                    Ok("no recursion, and only `for` loops".into())
                } else {
                    Ok(hows.join("; "))
                }
            }
        };
        own.insert(name.clone(), r);
    }

    let marked = |n: &str| env.fns.get(&n.sym()).is_some_and(|f| f.terminates) || env.impls.iter().chain(env.inherent.values()).any(|i| i.methods.iter().any(|m| m.terminates && impl_label(env, i, &m.name) == n));
    // combine with the callees, deepest first (cycles are already unknown)
    fn resolve(n: &str, nodes: &HashMap<String, Node>, own: &HashMap<String, Result<String, String>>, marked: &dyn Fn(&str) -> bool, memo: &mut HashMap<String, Termination>, visiting: &mut HashSet<String>) -> Termination {
        if let Some(t) = memo.get(n) {
            return t.clone();
        }
        let mut t = match &own[n] {
            Err(reason) => Termination::Unknown { reason: reason.clone() },
            Ok(how) => Termination::Proven { how: how.clone() },
        };
        if matches!(t, Termination::Proven { .. }) && visiting.insert(n.to_string()) {
            let mut assumed = BTreeSet::new();
            for c in &nodes[n].calls {
                if c == n {
                    continue;
                }
                match resolve(c, nodes, own, marked, memo, visiting) {
                    Termination::Proven { .. } => {}
                    Termination::Assumed { by } => assumed.extend(by),
                    Termination::Unknown { .. } => {
                        // a trait method by its short name (`next`, not `std.prelude.next`)
                        let shown = c.strip_prefix("trait method ").map_or(c.as_str(), crate::names::short);
                        t = Termination::Unknown { reason: format!("it calls `{shown}`, which may not terminate") };
                        break;
                    }
                }
            }
            if matches!(t, Termination::Proven { .. }) && !assumed.is_empty() {
                t = Termination::Assumed { by: assumed.into_iter().collect() };
            }
            visiting.remove(n);
        }
        if matches!(t, Termination::Unknown { .. }) && marked(n) {
            t = Termination::Assumed { by: vec![n.to_string()] };
        }
        memo.insert(n.to_string(), t.clone());
        t
    }
    let mut memo = HashMap::default();
    let names: Vec<String> = nodes.keys().cloned().collect();
    for n in &names {
        resolve(n, &nodes, &own, &marked, &mut memo, &mut HashSet::default());
    }

    // transparent: finishes, and cannot fail
    let mut transparent = HashMap::default();
    let mut changed = true;
    while changed {
        changed = false;
        for (name, f) in &env.fns {
            let name = name.as_str();
            if transparent.contains_key(name) {
                continue;
            }
            let assumed = match &memo[name] {
                Termination::Proven { .. } => false,
                Termination::Assumed { .. } => true,
                Termination::Unknown { .. } => continue,
            };
            if recursive.contains(name) {
                continue;
            }
            if cannot_fail(env, f, &|g| transparent.contains_key(g)) {
                transparent.insert(name.to_string(), assumed);
                changed = true;
            }
        }
    }
    let status = memo.into_iter().filter(|(k, _)| !k.starts_with("trait method ")).collect();
    Analysis { status, recursive: recursive.into_iter().filter(|k| !k.starts_with("trait method ")).collect(), transparent }
}
