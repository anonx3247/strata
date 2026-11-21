//! Regular expressions for the prover: translating a regex to SMT-LIB with
//! the interpreter's meaning, and deciding how two regexes relate.

use super::*;

/// The largest code point an SMT-LIB string can hold (the standard's
/// alphabet is U+0000..U+2FFFF; Z3 and cvc5 both stop there). Rust strings go
/// up to U+10FFFF, so a proof about strings covers the strings whose code
/// points are all at most this. The cross-check fuzzing is the backstop.
pub const MAX_CHAR: u32 = 0x2FFFF;

/// Translate a regular expression to an SMT-LIB regular expression, with
/// the interpreter's meaning: `matches` is `regex::Regex` on `^(?:re)$`,
/// a Unicode regex (`\d` is any decimal digit, `\w` and `\s` are Unicode
/// classes, `.` is any character but `\n`, `(?i)` is Unicode case folding).
/// The pattern is parsed by `regex-syntax` with the same settings, and its
/// HIR translated. `None` (an unknown) for anything whose meaning can't be
/// expressed exactly: word boundaries, multi-line anchors, anchors that
/// are not at the very start or end, byte classes, and literal characters
/// past `MAX_CHAR`.
#[cfg(test)]
pub(super) fn regex_to_smt(re: &str) -> Option<T> {
    hir_to_smt(&parse_regex(re)?, true, true, Mode::Exact, &mut 0)
}

/// How `matches(s, re)` is modelled: `Exact(r)` is `s in r`. With a class of
/// more than `MAX_RANGES` ranges (`\w`, `\p{L}`), which Z3 can't decide in
/// reasonable time, it is `Between(under, over)`: the class replaced by a
/// subset of its ranges, and by a superset with fewer gaps. Regex
/// operators are monotone, so `under <= re <= over`, and `matches` is any
/// value that is true on `under` and false outside `over`.
#[derive(Debug, PartialEq)]
pub enum RegexModel {
    Exact(T),
    Between(T, T),
}

/// Classes with more ranges than this are approximated (see `RegexModel`).
pub(super) const MAX_RANGES: usize = 24;

pub fn regex_model(re: &str) -> Option<RegexModel> {
    let hir = parse_regex(re)?;
    let mut widest = 0;
    let exact = hir_to_smt(&hir, true, true, Mode::Exact, &mut widest)?;
    if widest <= MAX_RANGES {
        return Some(RegexModel::Exact(exact));
    }
    Some(RegexModel::Between(hir_to_smt(&hir, true, true, Mode::Under, &mut 0)?, hir_to_smt(&hir, true, true, Mode::Over, &mut 0)?))
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Mode {
    Exact,
    Under,
    Over,
}

/// The pattern as the interpreter's `matches` reads it.
pub(super) fn parse_regex(re: &str) -> Option<regex_syntax::hir::Hir> {
    regex_syntax::ParserBuilder::new()
        // `regex::Regex::new`'s settings (regex-automata's `syntax::Config` defaults)
        .case_insensitive(false)
        .multi_line(false)
        .dot_matches_new_line(false)
        .crlf(false)
        .line_terminator(b'\n')
        .swap_greed(false)
        .ignore_whitespace(false)
        .unicode(true)
        .utf8(true)
        .nest_limit(250)
        .octal(false)
        .build()
        .parse(&format!("^(?:{re})$"))
        .ok()
}

/// A class's ranges, within what SMT strings can hold; in `Under` / `Over`
/// mode at most `MAX_RANGES` of them: a subset (ranges touching Latin-1
/// first, then the widest), or a superset (the smallest gaps filled).
pub(super) fn class_ranges(c: &regex_syntax::hir::ClassUnicode, mode: Mode) -> Vec<(u32, u32)> {
    let mut rs: Vec<(u32, u32)> = c.ranges().iter().filter(|r| r.start() as u32 <= MAX_CHAR).map(|r| (r.start() as u32, (r.end() as u32).min(MAX_CHAR))).collect();
    if rs.len() <= MAX_RANGES || mode == Mode::Exact {
        return rs;
    }
    match mode {
        Mode::Under => {
            let mut order: Vec<usize> = (0..rs.len()).collect();
            order.sort_by_key(|&i| (rs[i].0 > 0xFF, std::cmp::Reverse(rs[i].1 - rs[i].0), i));
            let mut keep: Vec<usize> = order.into_iter().take(MAX_RANGES).collect();
            keep.sort();
            keep.into_iter().map(|i| rs[i]).collect()
        }
        _ => {
            // fill the smallest gaps between neighbours until few enough ranges are left
            let mut gaps: Vec<(u32, usize)> = (1..rs.len()).map(|i| (rs[i].0 - rs[i - 1].1, i)).collect();
            gaps.sort();
            let mut fill = vec![false; rs.len()];
            for &(_, i) in gaps.iter().take(rs.len() - MAX_RANGES) {
                fill[i] = true;
            }
            let mut out: Vec<(u32, u32)> = vec![];
            for (i, r) in rs.drain(..).enumerate() {
                match out.last_mut() {
                    Some(last) if fill[i] => last.1 = r.1,
                    _ => out.push(r),
                }
            }
            out
        }
    }
}

/// How two patterns' languages relate (whole-text matches): `(a <= b,
/// b <= a, disjoint)`, each `false` when it does not hold or could not be
/// decided. Decided by `std.regex.relations`, on the product of the two
/// patterns' DFAs (over code points), run as compile-time code is (on the
/// JIT, cached between runs). Code points past `MAX_CHAR` are in no SMT
/// string, so what holds of every string holds of the SMT strings too.
pub fn regex_relations(a: &str, b: &str) -> (bool, bool, bool) {
    thread_local! {
        static STD: Option<Program> = crate::parser::parse_program("use std.regex.*\n").ok().map(|p| crate::resolve::resolve(&p).program);
        static DONE: std::cell::RefCell<HashMap<(String, String), (bool, bool, bool)>> = Default::default();
    }
    let key = (a.to_string(), b.to_string());
    if let Some(r) = DONE.with(|d| d.borrow().get(&key).copied()) {
        return r;
    }
    let call = Expr::Call(Box::new(Expr::Var(Symbol::new("std.regex.relations"))), vec![Expr::Str(a.to_string()), Expr::Str(b.to_string())]);
    let made = STD.with(|p| p.as_ref().and_then(|p| crate::derive::comptime_literals(p, &[call]).pop()));
    let r = match made {
        Some(Ok(Expr::Tuple(xs))) => match xs.as_slice() {
            [Expr::Bool(sub), Expr::Bool(sup), Expr::Bool(disjoint)] => (*sub, *sup, *disjoint),
            _ => (false, false, false),
        },
        _ => (false, false, false),
    };
    DONE.with(|d| d.borrow_mut().insert(key, r));
    r
}

pub(super) fn re_empty() -> T {
    "(str.to_re \"\")".into()
}

/// `at_start` / `at_end`: this part is certain to begin at the start (end
/// at the end) of the string, so `^` / `$` there match the empty string.
/// `widest`: the most ranges a class has (in `Exact` mode).
pub(super) fn hir_to_smt(h: &regex_syntax::hir::Hir, at_start: bool, at_end: bool, mode: Mode, widest: &mut usize) -> Option<T> {
    use regex_syntax::hir::{Class, HirKind, Look};
    let zero_start = |h: &regex_syntax::hir::Hir| matches!(h.kind(), HirKind::Empty | HirKind::Look(Look::Start));
    let zero_end = |h: &regex_syntax::hir::Hir| matches!(h.kind(), HirKind::Empty | HirKind::Look(Look::End));
    Some(match h.kind() {
        HirKind::Empty => re_empty(),
        HirKind::Literal(lit) => {
            let s = std::str::from_utf8(&lit.0).ok()?;
            if s.chars().any(|c| c as u32 > MAX_CHAR) {
                return None;
            }
            format!("(str.to_re {})", str_lit(s))
        }
        HirKind::Class(Class::Unicode(c)) => {
            // characters past MAX_CHAR are in no SMT string: dropped
            *widest = (*widest).max(c.ranges().len());
            let items: Vec<T> = class_ranges(c, mode)
                .into_iter()
                .map(|(lo, hi)| {
                    let (lo, hi) = (char::from_u32(lo).unwrap(), char::from_u32(hi).unwrap());
                    if lo == hi {
                        format!("(str.to_re {})", str_lit(&lo.to_string()))
                    } else {
                        format!("(re.range {} {})", str_lit(&lo.to_string()), str_lit(&hi.to_string()))
                    }
                })
                .collect();
            match items.len() {
                0 => "re.none".into(),
                1 => items[0].clone(),
                _ => format!("(re.union {})", items.join(" ")),
            }
        }
        // a class of bytes does not occur in Unicode mode
        HirKind::Class(Class::Bytes(_)) => return None,
        HirKind::Look(Look::Start) if at_start => re_empty(),
        HirKind::Look(Look::End) if at_end => re_empty(),
        HirKind::Look(_) => return None,
        // greedy or lazy, a repetition matches the same strings
        HirKind::Repetition(r) => {
            let sub = hir_to_smt(&r.sub, false, false, mode, widest)?;
            match (r.min, r.max) {
                (0, None) => format!("(re.* {sub})"),
                (1, None) => format!("(re.+ {sub})"),
                (n, None) => format!("(re.++ ((_ re.loop {n} {n}) {sub}) (re.* {sub}))"),
                (0, Some(1)) => format!("(re.opt {sub})"),
                (lo, Some(hi)) if lo <= hi => format!("((_ re.loop {lo} {hi}) {sub})"),
                _ => return None,
            }
        }
        HirKind::Capture(c) => hir_to_smt(&c.sub, at_start, at_end, mode, widest)?,
        HirKind::Concat(xs) => {
            let mut items = vec![];
            for (i, x) in xs.iter().enumerate() {
                let s = at_start && xs[..i].iter().all(zero_start);
                let e = at_end && xs[i + 1..].iter().all(zero_end);
                let t = hir_to_smt(x, s, e, mode, widest)?;
                if t != re_empty() {
                    items.push(t);
                }
            }
            match items.len() {
                0 => re_empty(),
                1 => items.pop().unwrap(),
                _ => format!("(re.++ {})", items.join(" ")),
            }
        }
        HirKind::Alternation(xs) => {
            let items: Vec<T> = xs.iter().map(|x| hir_to_smt(x, at_start, at_end, mode, widest)).collect::<Option<_>>()?;
            format!("(re.union {})", items.join(" "))
        }
    })
}

