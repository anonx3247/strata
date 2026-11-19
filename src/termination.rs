//! Termination: does a function always finish?
//!
//! Shown automatically, without annotations, in the common cases:
//! * no recursion, and only `for` loops (a list is finite);
//! * a `while a < b` loop (or `<=`, `>`, `>=`) whose bound `b - a` the solver
//!   proves goes down on every pass through the body;
//! * recursion where some argument is a piece of the same parameter, taken
//!   apart by a `match` (`[x, ..rest] => f(rest)`, `Node { left, .. } => f(left)`):
//!   values are finite, so this can't go on forever.
//!
//! A function also needs everything it calls to terminate. Anything else
//! (a `loop`, other recursion, mutual recursion) "may not terminate", unless
//! it is marked `terminates`, which is reported as assumed.

use crate::ast::*;
use crate::types::{texpr_head, Env};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};

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
}

fn trait_node(m: &str) -> String {
    format!("trait method {m}")
}

/// The label of an implementation's method, as `verify` shows it.
pub fn impl_label(imp: &ImplDecl, m: &str) -> String {
    format!("{}.{m}", crate::printer::ty(&imp.for_type))
}

fn calls_of(e: &Expr, env: &Env, out: &mut BTreeSet<String>) {
    crate::verify::walk(e, &mut |x| match x {
        Expr::Call(f, _) => {
            if let Expr::Var(n) = &**f {
                if env.fns.contains_key(n) {
                    out.insert(n.clone());
                } else if env.method_trait.contains_key(n) {
                    out.insert(trait_node(n));
                }
            }
        }
        Expr::Method(_, m, _) => {
            if env.fns.contains_key(m) {
                out.insert(m.clone());
            } else if env.method_trait.contains_key(m) {
                out.insert(trait_node(m));
            }
        }
        // a function passed as a value may be called
        Expr::Var(n) if env.fns.contains_key(n) => {
            out.insert(n.clone());
        }
        Expr::Var(n) if env.method_trait.contains_key(n) => {
            out.insert(trait_node(n));
        }
        _ => {}
    });
}

fn has_stmt(e: &Expr, pred: &dyn Fn(&Stmt) -> bool) -> bool {
    let mut found = false;
    crate::verify::walk(e, &mut |x| {
        if let Expr::Block(stmts, _) = x {
            found |= stmts.iter().any(pred);
        }
    });
    found
}

/// The loops of a body: `Ok(how they end)` or `Err(why one may not)`.
fn loops(env: &Env, f: &FnDecl, solver: Option<&str>) -> Result<Vec<String>, String> {
    if has_stmt(&f.body, &|s| matches!(s, Stmt::Loop(_))) {
        return Err("it has a `loop` (only `for` loops, and `while` loops whose bound goes down, are shown to end)".into());
    }
    if !has_stmt(&f.body, &|s| matches!(s, Stmt::While(..))) {
        return Ok(vec![]);
    }
    let Some(bin) = solver else { return Err("it has a `while` loop, and no SMT solver was found to check its bound".into()) };
    let mut vc = crate::smt::Vc::new(env);
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
            Expr::Var(x) => vars.get(x).cloned().unwrap_or_default(),
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
            Pattern::Bind(x) => {
                let piece = if self.assigned.contains(x) {
                    Piece::default()
                } else if depth > 0 {
                    Piece { within: whole.within.clone(), smaller: whole.within.clone() }
                } else {
                    whole.clone()
                };
                vars.insert(x.clone(), piece);
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
                CtorPat::Positional(ps) => ps.iter().for_each(|p| self.bind(p, None, whole, depth + 1, vars)),
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
                        v.insert(n.clone(), whole.clone());
                    }
                    self.bind(&a.pat, Some(s), &whole, 0, &mut v);
                    self.expr(&a.body, &v);
                }
            }
            Expr::Lambda(ps, body) => {
                let mut v = vars.clone();
                for p in ps {
                    v.remove(&p.name);
                }
                self.expr(body, &v);
            }
            Expr::Block(stmts, last) => {
                let mut v = vars.clone();
                for s in stmts {
                    match s {
                        Stmt::Let(p, _, x) => {
                            self.expr(x, &v);
                            let w = self.piece(x, &v);
                            self.bind(p, Some(x), &w, 0, &mut v);
                        }
                        Stmt::Var(x, _, init) => {
                            self.expr(init, &v);
                            v.remove(x);
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
                        Stmt::Expr(x) | Stmt::Assert(x) | Stmt::Loop(x) => self.expr(x, &v),
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
                    Expr::Interp(parts) => kids.extend(parts.iter().filter_map(|p| if let StrPart::Expr(x) = p { Some(x.clone()) } else { None })),
                    Expr::Except(x, name, fb) => {
                        self.expr(x, vars);
                        let mut v = vars.clone();
                        if let Some(n) = name {
                            v.remove(n);
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

fn assigned(e: &Expr) -> HashSet<String> {
    let mut out = HashSet::new();
    crate::verify::walk(e, &mut |x| {
        if let Expr::Mut(_) = x {
            if let Some(n) = crate::smt::place_root(x) {
                out.insert(n);
            }
        }
        if let Expr::Block(stmts, _) = x {
            for s in stmts {
                if let Stmt::Assign(place, _) = s {
                    let mut root = place;
                    while let Expr::Field(r, _) = root {
                        root = r;
                    }
                    if let Expr::Var(n) = root {
                        out.insert(n.clone());
                    }
                }
            }
        }
    });
    out
}

/// If every recursive call passes a smaller piece of the same parameter,
/// the name of that parameter.
fn structural(f: &FnDecl) -> Option<String> {
    let assigned = assigned(&f.body);
    let mut vars = HashMap::new();
    for (i, p) in f.params.iter().enumerate() {
        if !assigned.contains(&p.name) && !p.mutable {
            vars.insert(p.name.clone(), Piece { within: [i].into(), smaller: BTreeSet::new() });
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
    common.and_then(|c| c.into_iter().next()).map(|i| f.params[i].name.clone())
}

/// If every recursive call passes, in the same position, an integer smaller
/// than the parameter, which is not negative when the call is made (as in
/// `fact(n - 1)` under `n > 0`): that parameter's name. Checked by the solver.
fn int_recursion(env: &Env, f: &FnDecl, solver: Option<&str>) -> Option<String> {
    let bin = solver?;
    let mut vc = crate::smt::Vc::new(env);
    vc.function(f, &f.name);
    if vc.rec_calls.is_empty() {
        return None;
    }
    (0..f.params.len()).find(|i| vc.rec_calls.iter().all(|call| call.iter().any(|(j, k)| j == i && matches!(vc.solve(bin, *k), crate::smt::Outcome::Proven)))).map(|i| f.params[i].name.clone())
}

// ---- transparency ----

/// Builtins that always return (never fail) on well-typed arguments.
const TOTAL_BUILTINS: &[&str] = &[
    "len", "str_len", "contains", "get", "dict", "insert", "remove", "keys", "values", "items", "map_values", "set", "to_list", "union", "intersection", "difference", "zip", "enumerate",
    "sort", "sort_by", "reverse", "join", "trim", "parse_int", "map", "filter", "fold", "all", "any", "push", "take", "drop", "find", "count", "abs", "min", "max", "starts_with",
    "to_string", "sum",
];

/// Can evaluating `f`'s body fail (a runtime check, a hole)? Calls to other
/// functions are fine when those cannot fail either.
fn cannot_fail(env: &Env, f: &FnDecl, total: &dyn Fn(&str) -> bool) -> bool {
    if !f.requires.is_empty() || f.params.iter().any(|p| p.mutable) {
        return false;
    }
    let mut ok = true;
    let refined = |t: &str| env.types.get(t).is_some_and(|_| crate::smt::type_is_refined(env, &TypeExpr::Named(t.into(), vec![])));
    crate::verify::walk(&f.body, &mut |x| match x {
        // an early return would need its own unfolding
        Expr::Hole(_) | Expr::Mock(_) | Expr::Return(_) | Expr::Try(..) => ok = false,
        Expr::Bin(BinOp::Div | BinOp::Mod, _, d) => ok &= matches!(&**d, Expr::Int(n) if *n != 0),
        Expr::Ctor(c, _) => ok &= !refined(&env.ctors.get(c).cloned().unwrap_or_default()),
        Expr::Call(g, _) => {
            if let Expr::Var(n) = &**g {
                ok &= if env.fns.contains_key(n) { total(n) } else { !env.method_trait.contains_key(n) && TOTAL_BUILTINS.contains(&n.as_str()) };
            }
        }
        Expr::Method(_, n, _) => ok &= if env.fns.contains_key(n) { total(n) } else { !env.method_trait.contains_key(n) && TOTAL_BUILTINS.contains(&n.as_str()) },
        Expr::Block(stmts, _) => ok &= !stmts.iter().any(|s| matches!(s, Stmt::Assert(_))),
        _ => {}
    });
    ok
}

pub fn analyze(env: &Env, solver: Option<&str>) -> Analysis {
    let mut nodes: HashMap<String, Node> = HashMap::new();
    for (name, f) in &env.fns {
        let mut calls = BTreeSet::new();
        calls_of(&f.body, env, &mut calls);
        f.requires.iter().chain(&f.ensures).for_each(|c| calls_of(c, env, &mut calls));
        nodes.insert(name.clone(), Node { decl: Some(f.clone()), calls, loops: loops(env, f, solver) });
    }
    for imp in &env.impls {
        let Some(tr) = env.traits.get(&imp.trait_name) else { continue };
        for tm in &tr.methods {
            let decl = imp.methods.iter().find(|m| m.name == tm.decl.name).or(tm.default.then_some(&tm.decl));
            let Some(decl) = decl else { continue };
            let label = impl_label(imp, &tm.decl.name);
            let mut calls = BTreeSet::new();
            calls_of(&decl.body, env, &mut calls);
            let d = crate::verify::for_impl(imp, decl);
            nodes.insert(label.clone(), Node { decl: None, calls, loops: loops(env, &d, solver) });
            nodes.entry(trait_node(&tm.decl.name)).or_insert(Node { decl: None, calls: BTreeSet::new(), loops: Ok(vec![]) }).calls.insert(label);
        }
        let _ = texpr_head(&imp.for_type);
    }
    // trait methods no implementation defines
    let missing: Vec<String> = nodes.values().flat_map(|n| n.calls.iter().cloned()).filter(|c| !nodes.contains_key(c)).collect();
    for m in missing {
        nodes.insert(m, Node { decl: None, calls: BTreeSet::new(), loops: Ok(vec![]) });
    }

    // what each node reaches
    let reach = |start: &str| -> HashSet<String> {
        let mut seen = HashSet::new();
        let mut stack: Vec<String> = nodes[start].calls.iter().cloned().collect();
        while let Some(n) = stack.pop() {
            if seen.insert(n.clone()) {
                stack.extend(nodes[&n].calls.iter().cloned());
            }
        }
        seen
    };
    let reaches: HashMap<String, HashSet<String>> = nodes.keys().map(|k| (k.clone(), reach(k))).collect();
    let mutual = |n: &str| -> Option<String> { nodes[n].calls.iter().find(|c| c.as_str() != n && reaches[c.as_str()].contains(n)).cloned() };
    let recursive: HashSet<String> = nodes.keys().filter(|n| reaches[n.as_str()].contains(n.as_str())).cloned().collect();

    // each node's own verdict, ignoring what it calls
    let mut own: HashMap<String, Result<String, String>> = HashMap::new();
    for (name, n) in &nodes {
        let r = match &n.loops {
            Err(e) => Err(e.clone()),
            Ok(hows) => {
                if let Some(other) = mutual(name) {
                    Err(format!("it is recursive through `{}`", other.trim_start_matches("trait method ")))
                } else if n.calls.contains(name) {
                    match n.decl.as_ref().and_then(structural) {
                        Some(p) => Ok(format!("each recursive call gets a smaller piece of `{p}`")),
                        None => match n.decl.as_ref().and_then(|f| int_recursion(env, f, solver)) {
                            Some(p) => Ok(format!("each recursive call gets a smaller `{p}`, which is never negative")),
                            None => Err("it calls itself, and no argument gets smaller on every call (a smaller piece of a parameter, or a smaller non-negative integer)".into()),
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

    let marked = |n: &str| env.fns.get(n).is_some_and(|f| f.terminates) || env.impls.iter().any(|i| i.methods.iter().any(|m| m.terminates && impl_label(i, &m.name) == n));
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
                        let shown = c.trim_start_matches("trait method ");
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
    let mut memo = HashMap::new();
    let names: Vec<String> = nodes.keys().cloned().collect();
    for n in &names {
        resolve(n, &nodes, &own, &marked, &mut memo, &mut HashSet::new());
    }

    // transparent: finishes, and cannot fail
    let mut transparent = HashMap::new();
    let mut changed = true;
    while changed {
        changed = false;
        for (name, f) in &env.fns {
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
                transparent.insert(name.clone(), assumed);
                changed = true;
            }
        }
    }
    let status = memo.into_iter().filter(|(k, _)| !k.starts_with("trait method ")).collect();
    Analysis { status, recursive: recursive.into_iter().filter(|k| !k.starts_with("trait method ")).collect(), transparent }
}
