//! Static execution outline of a function: the calls, effect operations,
//! handlers, branches and loops in its body, in evaluation order. The viewer
//! expands these into execution paths from an entry point and resolves each
//! effect operation to the handler that answers it.

use crate::ast::*;
use crate::printer;
use serde::Serialize;
use std::collections::HashSet;

#[derive(Serialize, Debug, Clone)]
#[serde(tag = "k", rename_all = "snake_case")]
pub enum Step {
    /// A call to a user-defined function.
    Call { f: String, args: String },
    /// `if` / `match`: alternative paths.
    Branch { on: String, arms: Vec<ArmInfo> },
    /// A closure run for each element (`map`, `filter`, `fold`, ...).
    Each { via: String, over: String, steps: Vec<Step> },
    /// A trait method call: which implementation runs depends on the receiver's type.
    Dispatch { method: String, trait_name: String, args: String, targets: Vec<Target> },
}

#[derive(Serialize, Debug, Clone)]
pub struct Target {
    /// The implementing type, e.g. `Item`.
    pub ty: String,
    /// Key of the implementation's outline in the flow table.
    pub key: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct ArmInfo {
    pub label: String,
    pub steps: Vec<Step>,
}

const LOOPS: &[&str] = &["map", "filter", "fold", "all", "any", "find", "count", "sort_by", "map_values"];

fn short(s: String) -> String {
    let s = s.replace('\n', " ");
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > 48 { s.chars().take(47).collect::<String>() + "…" } else { s }
}

fn args_text(es: &[Expr]) -> String {
    short(es.iter().map(|e| printer::expr(e, 0)).collect::<Vec<_>>().join(", "))
}

pub struct Outliner<'a> {
    pub fns: &'a HashSet<String>,
    /// trait method -> (trait, implementations)
    pub methods: &'a std::collections::HashMap<String, (String, Vec<Target>)>,
}

impl Outliner<'_> {
    pub fn steps(&self, e: &Expr) -> Vec<Step> {
        let mut out = Vec::new();
        self.walk(e, &mut out);
        out
    }

    fn walk(&self, e: &Expr, out: &mut Vec<Step>) {
        match e {
            Expr::Call(f, args) => {
                let name = match &**f {
                    Expr::Var(n) => Some(n.clone()),
                    other => {
                        self.walk(other, out);
                        None
                    }
                };
                self.call(name.as_deref(), args, out);
            }
            Expr::Method(recv, m, args) => {
                // `xs.map(f)` is `map(xs, f)`
                let all: Vec<Expr> = std::iter::once((**recv).clone()).chain(args.iter().cloned()).collect();
                self.call(Some(m), &all, out);
            }
            Expr::If(c, t, f) => {
                self.walk(c, out);
                let arms = vec![
                    ArmInfo { label: format!("if {}", short(printer::expr(c, 0))), steps: self.steps(t) },
                    ArmInfo { label: "else".into(), steps: self.steps(f) },
                ];
                if arms.iter().any(|a| !a.steps.is_empty()) {
                    out.push(Step::Branch { on: short(printer::expr(c, 0)), arms });
                }
            }
            Expr::Match(s, arms) => {
                self.walk(s, out);
                let arms: Vec<ArmInfo> = arms.iter().map(|a| ArmInfo { label: printer::pat(&a.pat), steps: self.steps(&a.body) }).collect();
                if arms.iter().any(|a| !a.steps.is_empty()) {
                    out.push(Step::Branch { on: format!("match {}", short(printer::expr(s, 0))), arms });
                }
            }
            Expr::Block(stmts, last) => {
                for s in stmts {
                    match s {
                        Stmt::Comment(_) => {}
                        Stmt::Let(_, _, x) | Stmt::Var(_, _, x) | Stmt::Expr(x) | Stmt::Assert(x) => self.walk(x, out),
                        Stmt::Assign(a, b) | Stmt::While(a, b) | Stmt::For(_, a, b) => {
                            self.walk(a, out);
                            self.walk(b, out);
                        }
                        Stmt::Loop(b) => self.walk(b, out),
                        Stmt::Break | Stmt::Continue => {}
                    }
                }
                self.walk(last, out);
            }
            Expr::Lambda(_, b) => self.walk(b, out),
            Expr::List(xs) | Expr::Tuple(xs) => xs.iter().for_each(|x| self.walk(x, out)),
            Expr::Ctor(_, CtorArgs::Positional(xs)) => xs.iter().for_each(|x| self.walk(x, out)),
            Expr::Ctor(_, CtorArgs::Record(fs)) => fs.iter().for_each(|(_, x)| self.walk(x, out)),
            Expr::Ctor(_, CtorArgs::Update(b, fs)) => {
                self.walk(b, out);
                fs.iter().for_each(|(_, x)| self.walk(x, out));
            }
            Expr::Field(r, _) | Expr::Is(r, _) | Expr::Un(_, r) => self.walk(r, out),
            Expr::Bin(_, l, r) => {
                self.walk(l, out);
                self.walk(r, out);
            }
            _ => {}
        }
    }

    fn call(&self, name: Option<&str>, args: &[Expr], out: &mut Vec<Step>) {
        let is_loop = name.is_some_and(|n| LOOPS.contains(&n) && !self.fns.contains(n));
        if let Some((tr, targets)) = name.and_then(|n| self.methods.get(n)) {
            for a in args {
                self.walk(a, out);
            }
            out.push(Step::Dispatch { method: name.unwrap().to_string(), trait_name: tr.clone(), args: args_text(args), targets: targets.clone() });
            return;
        }
        if is_loop {
            // non-closure args evaluate first; the closure runs per element
            let mut inner = Vec::new();
            for a in args {
                match a {
                    Expr::Lambda(_, b) => self.walk(b, &mut inner),
                    Expr::Var(f) if self.fns.contains(f) => inner.push(Step::Call { f: f.clone(), args: "each element".into() }),
                    Expr::Var(f) if self.methods.contains_key(f) => {
                        let (tr, targets) = &self.methods[f];
                        inner.push(Step::Dispatch { method: f.clone(), trait_name: tr.clone(), args: "each element".into(), targets: targets.clone() })
                    }
                    other => self.walk(other, out),
                }
            }
            if !inner.is_empty() {
                let over = args.first().map(|a| short(printer::expr(a, 0))).unwrap_or_default();
                out.push(Step::Each { via: name.unwrap().to_string(), over, steps: inner });
            }
            return;
        }
        for a in args {
            self.walk(a, out);
        }
        if let Some(n) = name {
            if self.fns.contains(n) {
                out.push(Step::Call { f: n.to_string(), args: args_text(args) });
            }
        }
    }
}

/// Head type name of a type expression (`List[Line]` -> `List`).
pub fn head(t: &TypeExpr) -> Option<String> {
    match t {
        TypeExpr::Named(n, _) => Some(n.clone()),
        _ => None,
    }
}

/// Every named type mentioned in a type expression.
pub fn mentions(t: &TypeExpr, out: &mut Vec<String>) {
    match t {
        TypeExpr::Named(n, args) => {
            if !out.contains(n) {
                out.push(n.clone());
            }
            args.iter().for_each(|a| mentions(a, out));
        }
        TypeExpr::Fn(ps, r) => {
            ps.iter().for_each(|a| mentions(a, out));
            mentions(r, out);
        }
        TypeExpr::Unit => {}
        TypeExpr::Refined(b, _) => mentions(b, out),
    }
}
