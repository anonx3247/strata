//! Sets of integers as sorted, disjoint, inclusive intervals (in i128 so that
//! the bounds of i64 never overflow).

use crate::ast::{BinOp, Expr, UnOp};
pub type Ints = Vec<(i128, i128)>;
pub const MIN: i128 = i64::MIN as i128;
pub const MAX: i128 = i64::MAX as i128;

pub fn point(n: i128) -> Ints {
    vec![(n, n)]
}
fn norm(mut v: Ints) -> Ints {
    v.retain(|(a, b)| a <= b);
    v.sort();
    let mut out: Ints = Vec::new();
    for (a, b) in v {
        match out.last_mut() {
            Some((_, hi)) if a <= *hi + 1 => *hi = (*hi).max(b),
            _ => out.push((a, b)),
        }
    }
    out
}
pub fn or(a: &Ints, b: &Ints) -> Ints {
    norm([a.clone(), b.clone()].concat())
}
pub fn not(a: &Ints) -> Ints {
    let mut out = Vec::new();
    let mut lo = MIN;
    for (x, y) in a {
        if *x > lo {
            out.push((lo, x - 1));
        }
        lo = y + 1;
    }
    if lo <= MAX {
        out.push((lo, MAX));
    }
    out
}
pub fn and(a: &Ints, b: &Ints) -> Ints {
    not(&or(&not(a), &not(b)))
}
pub fn contains(s: &Ints, lo: i128, hi: i128) -> bool {
    s.iter().any(|(a, b)| *a <= lo && hi <= *b)
}
/// The integers `it` for which a condition holds, when it only compares
/// `it` with constants (combined with `&&`, `||`, `!`, `==>`).
pub fn of_pred(e: &Expr) -> Option<Ints> {
    of_pred_on(e, &|e| matches!(e, Expr::Var(x) if x == "it"))
}

/// The lengths for which a condition on `len(it)` holds (never negative).
pub fn of_len_pred(e: &Expr) -> Option<Ints> {
    let is_len = |e: &Expr| match e {
        Expr::Call(f, a) => matches!(&**f, Expr::Var(n) if n == "len") && matches!(a.as_slice(), [Expr::Var(x)] if x == "it"),
        Expr::Method(r, m, a) => m == "len" && a.is_empty() && matches!(&**r, Expr::Var(x) if x == "it"),
        _ => false,
    };
    Some(and(&of_pred_on(e, &is_len)?, &vec![(0, MAX)]))
}

/// A superset of the integers for which a condition on `it` holds: as
/// `of_pred`, with the parts it can't read (`is_prime(it)`) taken as
/// anything. Steers generators; the condition itself is still checked.
pub fn hull(e: &Expr) -> Ints {
    hull_on(e, &of_pred)
}

/// The same for the lengths of `it`.
pub fn len_hull(e: &Expr) -> Ints {
    and(&hull_on(e, &of_len_pred), &vec![(0, MAX)])
}

fn hull_on(e: &Expr, exact: &dyn Fn(&Expr) -> Option<Ints>) -> Ints {
    if let Some(s) = exact(e) {
        return s;
    }
    match e {
        Expr::Bin(BinOp::And, l, r) => and(&hull_on(l, exact), &hull_on(r, exact)),
        Expr::Bin(BinOp::Or, l, r) => or(&hull_on(l, exact), &hull_on(r, exact)),
        _ => vec![(MIN, MAX)],
    }
}

fn of_pred_on(e: &Expr, subject: &dyn Fn(&Expr) -> bool) -> Option<Ints> {
    let of_pred = |e: &Expr| of_pred_on(e, subject);
    let konst = |e: &Expr| match e {
        Expr::Int(n) => Some(*n as i128),
        Expr::BigInt(d) => d.parse::<i128>().ok(),
        Expr::Un(UnOp::Neg, x) => match &**x {
            Expr::Int(n) => Some(-(*n as i128)),
            _ => None,
        },
        _ => None,
    };
    let is_it = subject;
    Some(match e {
        Expr::Bool(true) => vec![(MIN, MAX)],
        Expr::Bool(false) => vec![],
        Expr::Un(UnOp::Not, x) => not(&of_pred(x)?),
        Expr::Bin(BinOp::And, l, r) => and(&of_pred(l)?, &of_pred(r)?),
        Expr::Bin(BinOp::Or, l, r) => or(&of_pred(l)?, &of_pred(r)?),
        Expr::Bin(BinOp::Implies, l, r) => or(&not(&of_pred(l)?), &of_pred(r)?),
        Expr::Bin(op, l, r) => {
            use BinOp::*;
            let (op, c) = if is_it(l) {
                (*op, konst(r)?)
            } else if is_it(r) {
                let flipped = match op {
                    Lt => Gt,
                    Le => Ge,
                    Gt => Lt,
                    Ge => Le,
                    o => *o,
                };
                (flipped, konst(l)?)
            } else {
                return None;
            };
            match op {
                Lt => vec![(MIN, c - 1)],
                Le => vec![(MIN, c)],
                Gt => vec![(c + 1, MAX)],
                Ge => vec![(c, MAX)],
                Eq => point(c),
                Ne => not(&point(c)),
                _ => return None,
            }
        }
        _ => return None,
    })
}
