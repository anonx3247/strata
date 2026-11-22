//! Which values outlive what made them: the code generator's escape
//! analysis, for values it can keep on the stack.
//!
//! Functions borrow their arguments (see `Scope` in llvm.rs), so a value
//! passed to a function is still the caller's afterwards, unless the function
//! keeps it: puts it in its result, in another value, in a closure or a task,
//! or passes it on to a function that keeps it. `Kept` says, for each
//! function of the program and each parameter, whether it may keep it.
//!
//! The analysis is syntactic and conservative: a variable escapes unless
//! every use of it is one known to borrow: a field read (`p.x`), an `is`
//! test, a comparison, a `match` that binds no name to the whole value, or
//! an argument of the program's function whose parameter is not kept.
//! Anything else (a copy into another variable, a method or trait call, a
//! closure or task mentioning it, a `mut` argument, the value of a block or
//! a branch, a built-in) counts as escaping.

use crate::ast::*;
use crate::fx::HashMap;

/// For each function (by the name calls use), whether it may keep each parameter.
#[derive(Default)]
pub struct Kept(HashMap<String, Vec<bool>>);

impl Kept {
    /// The program's functions' parameters kept, found together: a function
    /// keeps a parameter if its body (or contracts) lets it escape, with the
    /// others' parameters assumed not kept until shown to be (so recursion
    /// alone keeps nothing).
    pub fn of(fns: &HashMap<String, &FnDecl>) -> Kept {
        let mut kept = Kept(fns.iter().map(|(n, f)| (n.clone(), vec![false; f.params.len()])).collect());
        loop {
            let mut changed = false;
            for (n, f) in fns {
                for (i, p) in f.params.iter().enumerate() {
                    if kept.0[n][i] {
                        continue;
                    }
                    let parts = f.requires.iter().chain(&f.ensures).chain(std::iter::once(&f.body));
                    if parts.into_iter().any(|e| escapes(e, &p.name, &kept)) {
                        kept.0.get_mut(n).unwrap()[i] = true;
                        changed = true;
                    }
                }
            }
            if !changed {
                return kept;
            }
        }
    }

    /// Whether calling `f` may keep its argument `i` (yes for anything not
    /// the program's function).
    fn keeps(&self, f: &str, i: usize) -> bool {
        self.0.get(f).is_none_or(|ps| ps.get(i).copied().unwrap_or(true))
    }
}

/// Whether variable `x` may escape `e` (the code after its binding).
pub fn escapes(e: &Expr, x: &Symbol, kept: &Kept) -> bool {
    let mut u = Uses { x, kept, escaped: false };
    u.expr(e);
    u.escaped
}

/// Whether `x`, bound by statement `stmts[i]`, may escape the rest of its block.
pub fn escapes_after(stmts: &[Stmt], i: usize, last: &Expr, x: &Symbol, kept: &Kept) -> bool {
    let mut u = Uses { x, kept, escaped: false };
    for s in &stmts[i + 1..] {
        walk_stmt(&mut u, s);
    }
    u.expr(last);
    u.escaped
}

struct Uses<'a> {
    x: &'a Symbol,
    kept: &'a Kept,
    escaped: bool,
}

impl Uses<'_> {
    fn is_x(&self, e: &Expr) -> bool {
        matches!(e, Expr::Var(v) if v == self.x)
    }

    /// Whether a pattern binds a name to the whole value it matches.
    fn binds_whole(p: &Pattern) -> bool {
        match p {
            Pattern::Bind(_) => true,
            Pattern::Or(ps) => ps.iter().any(Self::binds_whole),
            Pattern::For(a, ..) => Self::binds_whole(a),
            _ => false,
        }
    }
}

impl Visit for Uses<'_> {
    fn expr(&mut self, e: &Expr) {
        if self.escaped {
            return;
        }
        match e {
            // any use not handled below
            Expr::Var(v) if v == self.x => self.escaped = true,
            // reads
            Expr::Field(r, _) | Expr::Is(r, _) if self.is_x(r) => {}
            Expr::Bin(op, l, r) if !matches!(op, BinOp::And | BinOp::Or | BinOp::Implies) => {
                for s in [l, r] {
                    if !self.is_x(s) {
                        self.expr(s);
                    }
                }
            }
            // an argument of a function that borrows it
            Expr::Call(f, args) if matches!(&**f, Expr::Var(_)) => {
                let Expr::Var(name) = &**f else { unreachable!() };
                for (i, a) in args.iter().enumerate() {
                    if !(self.is_x(a) && !self.kept.keeps(name.as_str(), i)) {
                        self.expr(a);
                    }
                }
            }
            Expr::Match(s, binding, arms) if self.is_x(s) => {
                if binding.is_some() || arms.iter().any(|a| Self::binds_whole(&a.pat)) {
                    self.escaped = true;
                    return;
                }
                for a in arms {
                    self.pattern(&a.pat);
                    self.expr(&a.body);
                }
            }
            _ => walk_expr(self, e),
        }
    }
}
