//! Regular expressions for the prover: their SMT-LIB model and how two
//! relate, both from `std.regex` (run as compile-time code).

use super::*;

/// The largest code point an SMT-LIB string can hold (the standard's
/// alphabet is U+0000..U+2FFFF; Z3 and cvc5 both stop there). Rust strings go
/// up to U+10FFFF, so a proof about strings covers the strings whose code
/// points are all at most this. The cross-check fuzzing is the backstop.
pub const MAX_CHAR: u32 = 0x2FFFF;

/// The exact translation of `re` (`std.regex.smt_exact`), however large
/// its classes.
#[cfg(test)]
pub(super) fn regex_to_smt(re: &str) -> Option<T> {
    match crate::stdlib::regex_value("smt_exact", vec![Expr::Str(re.to_string())]).ok()? {
        Expr::Ctor(c, CtorArgs::Positional(xs)) if c.as_str().ends_with("Some") => match xs.as_slice() {
            [Expr::Str(t)] => Some(t.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// How `matches(s, re)` is modelled (`std.regex.SmtModel`): `Exact(r)` is
/// `s in r`. With a class of more than `MAX_RANGES` ranges (`\w`,
/// `\p{L}`), which Z3 can't decide in reasonable time, it is
/// `Between(under, over)`: the class replaced by a subset of its ranges,
/// and by a superset with fewer gaps. Regex operators are monotone, so
/// `under <= re <= over`, and `matches` is any value that is true on
/// `under` and false outside `over`.
#[derive(Debug, PartialEq)]
pub enum RegexModel {
    Exact(T),
    Between(T, T),
}

/// Classes with more ranges than this are approximated (`std.regex.max_smt_ranges`).
#[cfg(test)]
pub(super) const MAX_RANGES: usize = 24;

/// The prover's model of `matches(s, re)`: `std.regex.smt_model`, run as
/// compile-time code (cached), with `matches`' meaning (a Unicode regex
/// matching the whole text: `\d` any decimal digit, `\w` and `\s`
/// Unicode classes, `.` any character but `\n`, `(?i)` Unicode case
/// folding); `None` (an unknown) for anything whose meaning can't be
/// written exactly: word boundaries, line anchors, anchors not certain to
/// be at the very start or end, and literal characters past `MAX_CHAR`.
pub fn regex_model(re: &str) -> Option<RegexModel> {
    let str_of = |x: &Expr| if let Expr::Str(t) = x { Some(t.clone()) } else { None };
    let Expr::Ctor(c, CtorArgs::Positional(xs)) = crate::stdlib::regex_value("smt_model", vec![Expr::Str(re.to_string())]).ok()? else { return None };
    if !c.as_str().ends_with("Some") {
        return None;
    }
    let [Expr::Ctor(kind, CtorArgs::Positional(parts))] = xs.as_slice() else { return None };
    match (kind.as_str().rsplit('.').next(), parts.as_slice()) {
        (Some("ExactModel"), [r]) => Some(RegexModel::Exact(str_of(r)?)),
        (Some("Between"), [u, o]) => Some(RegexModel::Between(str_of(u)?, str_of(o)?)),
        _ => None,
    }
}

/// How two patterns' languages relate (whole-text matches): `(a <= b,
/// b <= a, disjoint)`, each `false` when it does not hold or could not be
/// decided. Decided by `std.regex.relations`, on the product of the two
/// patterns' DFAs (over code points), run as compile-time code
/// (`stdlib::regex_value`). Code points past `MAX_CHAR` are in no SMT
/// string, so what holds of every string holds of the SMT strings too.
pub fn regex_relations(a: &str, b: &str) -> (bool, bool, bool) {
    match crate::stdlib::regex_value("relations", vec![Expr::Str(a.to_string()), Expr::Str(b.to_string())]) {
        Ok(Expr::Tuple(xs)) => match xs.as_slice() {
            [Expr::Bool(sub), Expr::Bool(sup), Expr::Bool(disjoint)] => (*sub, *sup, *disjoint),
            _ => (false, false, false),
        },
        _ => (false, false, false),
    }
}
