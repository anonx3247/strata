//! Sets of floats described by a condition that compares `it` with
//! constants. Between two consecutive constants the condition cannot change,
//! so testing one point per region decides it exactly.

use crate::ast::{BinOp, Expr, UnOp};

#[derive(Clone, Debug)]
pub struct Reals {
    /// The constants the conditions compare with.
    pub cuts: Vec<f64>,
    /// All must hold (`negated`: the set is the complement).
    pub preds: Vec<Expr>,
    pub negated: bool,
}

impl Reals {
    pub fn holds(&self, x: f64) -> bool {
        self.preds.iter().all(|p| eval(p, x).unwrap_or(false)) != self.negated
    }
    pub fn and(mut self, other: Reals) -> Option<Reals> {
        if self.negated || other.negated {
            return None;
        }
        self.cuts.extend(other.cuts);
        self.preds.extend(other.preds);
        Some(self)
    }
    pub fn not(mut self) -> Reals {
        self.negated = !self.negated;
        self
    }
}

pub fn of_pred(e: &Expr) -> Option<Reals> {
    eval(e, 0.0)?;
    let mut cuts = Vec::new();
    consts(e, &mut cuts);
    Some(Reals { cuts, preds: vec![e.clone()], negated: false })
}

fn konst(e: &Expr) -> Option<f64> {
    match e {
        Expr::Float(f) => Some(*f),
        Expr::Int(n) => Some(*n as f64),
        Expr::Un(UnOp::Neg, x) => konst(x).map(|v| -v),
        _ => None,
    }
}

fn consts(e: &Expr, out: &mut Vec<f64>) {
    match e {
        Expr::Un(_, x) => consts(x, out),
        Expr::Bin(_, l, r) => {
            if let Some(c) = konst(l).or(konst(r)) {
                out.push(c);
            }
            consts(l, out);
            consts(r, out);
        }
        _ => {}
    }
}

/// The condition at `it = x`, or `None` if it is not of a form we decide.
fn eval(e: &Expr, x: f64) -> Option<bool> {
    let is_it = |e: &Expr| matches!(e, Expr::Var(v) if v == "it");
    Some(match e {
        Expr::Bool(b) => *b,
        Expr::Un(UnOp::Not, a) => !eval(a, x)?,
        Expr::Bin(BinOp::And, l, r) => eval(l, x)? & eval(r, x)?,
        Expr::Bin(BinOp::Or, l, r) => eval(l, x)? | eval(r, x)?,
        Expr::Bin(BinOp::Implies, l, r) => !eval(l, x)? | eval(r, x)?,
        Expr::Bin(op, l, r) => {
            let (a, b) = match (is_it(l), is_it(r)) {
                (true, false) => (x, konst(r)?),
                (false, true) => (konst(l)?, x),
                _ => return None,
            };
            use BinOp::*;
            match op {
                Lt => a < b,
                Le => a <= b,
                Gt => a > b,
                Ge => a >= b,
                Eq => a == b,
                Ne => a != b,
                _ => return None,
            }
        }
        _ => return None,
    })
}
